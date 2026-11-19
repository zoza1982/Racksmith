//! `cargo xtask spec`: nothing designed is missing silently.
//!
//! Checks `requirements.toml` against the system design (every leaf section classified, titles
//! in sync), the M1 plan (every tracking reference an open task), the code (entry points
//! defined, every input of a `Deserialize` or clap type classified) and the test listing (every
//! named test exists), then keeps the generated coverage page `docs/requirements.md` in sync.
//! See ADR 0035.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context as _, Result, bail, ensure};

use crate::design_sections::{self, DESIGN_FILE};
use crate::plan_tasks::{PLAN_FILE, PlanTasks};
use crate::requirements::{Facts, REQUIREMENTS_FILE, Report, Requirements};
use crate::rust_items::{self, ProductCode};
use crate::{term, test_listing, workspace};

/// The generated coverage page, relative to the workspace root.
const COVERAGE_FILE: &str = "docs/requirements.md";

/// Packages whose names start with this are product crates, whose inputs, items and tests
/// count.
pub(crate) const PRODUCT_PREFIX: &str = "racksmith-";

/// The pseudo-types of the inputs that no derive declares (see
/// [`rust_items::undeclared_inputs`]); a real input type may not take these names.
const UNDECLARED_INPUT_KINDS: [&str; 2] = ["route", "env"];

/// Runs every completeness check and verifies (or with `fix`, rewrites) the coverage page.
///
/// # Errors
///
/// Returns an error if an input file cannot be read or parsed, the tests do not build, nothing
/// was examined, any gap is found, or (without `fix`) the coverage page is stale.
pub(crate) fn run(root: &Path, fix: bool) -> Result<()> {
    let sections = design_sections::leaf_sections(&read(root, DESIGN_FILE)?)?;
    let tasks = PlanTasks::parse(&read(root, PLAN_FILE)?)?;
    let requirements = Requirements::parse(&read(root, REQUIREMENTS_FILE)?)?;
    let product = load_product_code(root)?;
    let definitions: BTreeMap<String, BTreeSet<String>> = product
        .iter()
        .map(|(package, code)| (package.clone(), rust_items::definitions(code)))
        .collect();
    let inputs = input_keys(&product)?;
    let listing = test_listing::load(root)?;
    let facts = Facts {
        sections: &sections,
        inputs: &inputs.keys,
        tasks: &tasks,
        definitions: &definitions,
        tests: &listing.tests,
        ignored_tests: &listing.ignored,
        lax_inputs: &inputs.lax,
    };
    ensure_examined(&facts)?;
    let report = requirements.check(&facts);
    term::info(&summary(&facts, &report))?;
    if !report.findings.is_empty() {
        for finding in &report.findings {
            term::error(&finding.to_string())?;
        }
        bail!(
            "{} completeness gap(s) in {REQUIREMENTS_FILE}; every design section and input must \
             be classified (see ADR 0035)",
            report.findings.len()
        );
    }
    sync_coverage_page(root, &requirements.render(&facts), fix)
}

fn read(root: &Path, file: &str) -> Result<String> {
    fs::read_to_string(root.join(file)).with_context(|| format!("failed to read {file}"))
}

/// The non-test code of every product crate, by package name.
fn load_product_code(root: &Path) -> Result<Vec<(String, Vec<ProductCode>)>> {
    workspace::load_crates(root)?
        .into_iter()
        .filter(|krate| krate.name.starts_with(PRODUCT_PREFIX))
        .map(|krate| {
            let sources = workspace::load_sources(root, &krate.dir)?;
            Ok((krate.name, rust_items::product_code(&sources)))
        })
        .collect()
}

/// The inputs of the product crates.
#[derive(Debug, Default, PartialEq, Eq)]
struct Inputs {
    /// Every input, as `<package>::<Type>.<member>`.
    keys: BTreeSet<String>,
    /// `Deserialize` types with named fields that accept unknown fields, as
    /// `<package>::<Type>`.
    lax: BTreeSet<String>,
}

/// Every input of the product crates: the members of their derived input types, their router
/// path parameters and the environment variables they read.
///
/// # Errors
///
/// Returns an error if one package has two input types of the same name, whose keys would
/// collide, an input type takes the name of an undeclared input kind (`route`, `env`), or the
/// name of an undeclared input cannot be read from the code.
fn input_keys(product: &[(String, Vec<ProductCode>)]) -> Result<Inputs> {
    let mut inputs = Inputs::default();
    for (package, code) in product {
        let mut names = BTreeSet::new();
        for input in rust_items::input_types(code) {
            ensure!(
                names.insert(input.name.clone()),
                "{package} has two input types named `{}`; their [inputs] keys would collide — \
                 rename one",
                input.name
            );
            ensure!(
                !UNDECLARED_INPUT_KINDS.contains(&input.name.as_str()),
                "{package} has an input type named `{}`, which collides with the keys of \
                 undeclared inputs — rename it",
                input.name
            );
            if input.accepts_unknown_fields {
                inputs.lax.insert(format!("{package}::{}", input.name));
            }
            inputs.keys.extend(
                input
                    .members
                    .iter()
                    .map(|member| format!("{package}::{}.{member}", input.name)),
            );
        }
        match rust_items::undeclared_inputs(code) {
            Ok(keys) => inputs
                .keys
                .extend(keys.into_iter().map(|key| format!("{package}::{key}"))),
            Err(problems) => bail!(
                "{package}: inputs whose names cannot be read from the code:\n  {}",
                problems.join("\n  ")
            ),
        }
    }
    Ok(inputs)
}

/// Fails if there was nothing to check, which means the check looked in the wrong place.
fn ensure_examined(facts: &Facts<'_>) -> Result<()> {
    ensure!(
        !facts.sections.is_empty(),
        "spec: found no numbered sections in {DESIGN_FILE}"
    );
    ensure!(
        !facts.inputs.is_empty(),
        "spec: found no fields or variants of Deserialize or clap types in the product crates"
    );
    ensure!(
        !facts.tests.is_empty(),
        "spec: the test listing is empty; is `cargo test -- --list` output being parsed?"
    );
    Ok(())
}

fn summary(facts: &Facts<'_>, report: &Report) -> String {
    format!(
        "spec: classified {}/{} design sections and {}/{} inputs; resolved {} tests (of {} \
         listed) and {} tasks (of {} in the plan)",
        report.sections_classified,
        facts.sections.len(),
        report.inputs_classified,
        facts.inputs.len(),
        report.tests_resolved,
        facts.tests.len(),
        report.tasks_resolved,
        facts.tasks.len(),
    )
}

/// Compares the coverage page with `page`, rewriting it when `fix` is set.
fn sync_coverage_page(root: &Path, page: &str, fix: bool) -> Result<()> {
    let path = root.join(COVERAGE_FILE);
    let current = fs::read_to_string(&path).unwrap_or_default();
    if current == page {
        term::info(&format!("spec: {COVERAGE_FILE} is up to date"))?;
        return Ok(());
    }
    if !fix {
        bail!("{COVERAGE_FILE} is out of date — run `cargo xtask spec --fix`");
    }
    fs::write(&path, page).with_context(|| format!("failed to write {COVERAGE_FILE}"))?;
    term::info(&format!("spec: regenerated {COVERAGE_FILE}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::design_sections::Section;
    use crate::workspace::SourceFile;

    fn product(package: &str, text: &str) -> (String, Vec<ProductCode>) {
        let file = SourceFile {
            path: PathBuf::from("src/lib.rs"),
            is_test_code: false,
            text: text.to_owned(),
        };
        (package.to_owned(), rust_items::product_code(&[file]))
    }

    #[test]
    fn input_keys_name_the_package_type_and_member() {
        let inputs = input_keys(&[
            product(
                "racksmith-api",
                "#[derive(Deserialize)]\nstruct Req { mac: String }",
            ),
            product(
                "racksmith-api",
                "fn router() -> Router { Router::new().route(\"/machines/{id}\", get(one)) }",
            ),
            product(
                "racksmith-cli",
                "#[derive(Subcommand)]\nenum Command { Serve { listen: Addr }, Version }\n\
                 fn main() { let filter = EnvFilter::builder().from_env_lossy(); }",
            ),
        ])
        .unwrap();
        let expected: BTreeSet<String> = [
            "racksmith-api::Req.mac",
            "racksmith-api::route./machines/{id}.id",
            "racksmith-cli::Command.Serve",
            "racksmith-cli::Command.Serve.listen",
            "racksmith-cli::Command.Version",
            "racksmith-cli::env.RUST_LOG",
        ]
        .map(str::to_owned)
        .into();
        assert_eq!(inputs.keys, expected);
        assert_eq!(
            inputs.lax,
            ["racksmith-api::Req".to_owned()].into(),
            "Req has no deny_unknown_fields"
        );
    }

    #[test]
    fn an_input_type_may_not_take_an_undeclared_kind_name() {
        let error = input_keys(&[product(
            "racksmith-api",
            "#[derive(Deserialize)]\n#[serde(deny_unknown_fields)]\n\
             #[expect(non_camel_case_types)]\nstruct env { x: u8 }",
        )])
        .unwrap_err()
        .to_string();
        assert!(error.contains("collides with the keys"), "{error}");
    }

    #[test]
    fn an_environment_read_without_a_literal_name_is_an_error() {
        let error = input_keys(&[product("racksmith-cli", "fn f(n: &str) { env::var(n); }")])
            .unwrap_err()
            .to_string();
        assert!(error.contains("cannot be read from the code"), "{error}");
    }

    #[test]
    fn colliding_input_type_names_are_an_error() {
        let text = "mod a {\n#[derive(Deserialize)]\nstruct Req { x: u8 }\n}\n\
                    mod b {\n#[derive(Deserialize)]\nstruct Req { y: u8 }\n}";
        let error = input_keys(&[product("racksmith-api", text)])
            .unwrap_err()
            .to_string();
        assert!(error.contains("two input types named `Req`"), "{error}");
    }

    #[test]
    fn a_run_that_examined_nothing_fails() {
        let found_sections = vec![Section {
            key: "1".into(),
            title: "t".into(),
        }];
        let found_inputs: BTreeSet<String> = ["racksmith-api::Req.mac".to_owned()].into();
        let found_tests: BTreeSet<String> = ["racksmith-api::t".to_owned()].into();
        let tasks = PlanTasks::default();
        let definitions = BTreeMap::new();
        let no_sections: Vec<Section> = Vec::new();
        let none = BTreeSet::new();
        let check = |sections: &[Section], inputs: &BTreeSet<String>, tests: &BTreeSet<String>| {
            ensure_examined(&Facts {
                sections,
                inputs,
                tasks: &tasks,
                definitions: &definitions,
                tests,
                ignored_tests: &none,
                lax_inputs: &none,
            })
        };
        assert!(
            check(&found_sections, &found_inputs, &found_tests).is_ok(),
            "complete"
        );
        assert!(
            check(&no_sections, &found_inputs, &found_tests).is_err(),
            "no sections"
        );
        assert!(
            check(&found_sections, &none, &found_tests).is_err(),
            "no inputs"
        );
        assert!(
            check(&found_sections, &found_inputs, &none).is_err(),
            "no tests"
        );
    }
}
