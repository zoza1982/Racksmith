//! Rule 7: every designed subsystem is wired or planned, never built and left uncalled.
//!
//! A `wired` subsystem's entry is defined in its crate's non-test sources and used (outside
//! tests, comments and strings, on identifier boundaries) by its consumer's non-test sources.
//! The consumer must depend on the defining crate and must not define an item of the same name,
//! which would make the use its own. Only the composition root may consume what it defines, and
//! then the use must be in its binary targets. A `planned` subsystem is tracked by open plan
//! tasks and its entry appears nowhere in the workspace, tests included, so there is no
//! half-wired code. See ADR 0035.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::architecture::{Architecture, Subsystem, SubsystemStatus};
use crate::plan_tasks::PlanTasks;
use crate::rust_items::{self, ProductCode};
use crate::tokens::{self, Token};
use crate::violation::Violation;
use crate::workspace::SourceFile;

/// The sources of one workspace crate, as rule 7 needs them.
#[derive(Debug)]
pub(crate) struct CrateSources<'a> {
    /// Package name.
    pub(crate) name: &'a str,
    /// Whether the crate is the composition root: the single highest-layer product crate,
    /// building a binary. Only it may consume a subsystem it defines.
    pub(crate) is_composition_root: bool,
    /// The source files of its binary targets: `src/main.rs` and `src/bin/`, or every file of
    /// a crate without a library target.
    pub(crate) binary_files: Vec<PathBuf>,
    /// The workspace crates it declares as normal dependencies.
    pub(crate) dependencies: Vec<&'a str>,
    /// Every source file, test code included.
    pub(crate) files: &'a [SourceFile],
}

/// The product view of one crate.
struct CrateView<'a> {
    sources: &'a CrateSources<'a>,
    /// Product code (test-only items and files removed).
    code: Vec<ProductCode>,
}

/// The views of the workspace that the rule compares entries against.
struct Index<'a> {
    /// Product code, by package.
    product: BTreeMap<&'a str, CrateView<'a>>,
    /// Every file's tokens, test code included.
    all: Vec<(PathBuf, Vec<Token>)>,
}

/// Checks every `[subsystems.*]` entry of `architecture`.
pub(crate) fn check(
    architecture: &Architecture,
    plan: &PlanTasks,
    crates: &[CrateSources<'_>],
) -> Vec<Violation> {
    if architecture.subsystems.is_empty() {
        return Vec::new();
    }
    let index = Index {
        product: crates
            .iter()
            .map(|krate| {
                let code = rust_items::product_code(krate.files);
                (
                    krate.name,
                    CrateView {
                        sources: krate,
                        code,
                    },
                )
            })
            .collect(),
        all: crates
            .iter()
            .flat_map(|krate| krate.files.iter())
            .map(|file| {
                let tokens = tokens::tokenize(&crate::source_text::split(&file.text));
                (file.path.clone(), tokens)
            })
            .collect(),
    };
    let mut violations = Vec::new();
    for (name, subsystem) in &architecture.subsystems {
        violations.extend(check_one(name, subsystem, plan, &index));
    }
    violations
}

fn check_one(
    name: &str,
    subsystem: &Subsystem,
    plan: &PlanTasks,
    index: &Index<'_>,
) -> Vec<Violation> {
    let mut violations = field_violations(name, subsystem);
    let entry = subsystem.entry.as_str();
    if tokens::tokenize_str(entry) != [entry] || !is_identifier(entry) {
        violations.push(Violation::SubsystemEntryNotIdentifier {
            subsystem: name.to_owned(),
            entry: entry.to_owned(),
        });
        return violations;
    }
    match subsystem.status {
        SubsystemStatus::Wired => {
            if let (Some(krate), Some(consumer)) = (&subsystem.krate, &subsystem.consumer) {
                violations.extend(wired(name, entry, krate, consumer, index));
            }
        }
        SubsystemStatus::Planned => {
            violations.extend(planned(name, entry, subsystem, plan, index));
        }
    }
    violations
}

/// Required fields that are absent or empty, and forbidden fields that are present.
fn field_violations(name: &str, subsystem: &Subsystem) -> Vec<Violation> {
    let wired = subsystem.status == SubsystemStatus::Wired;
    let fields = [
        (
            "crate",
            subsystem
                .krate
                .as_ref()
                .map(|krate| !krate.trim().is_empty()),
            wired,
        ),
        (
            "consumer",
            subsystem
                .consumer
                .as_ref()
                .map(|consumer| !consumer.trim().is_empty()),
            wired,
        ),
        (
            "tasks",
            subsystem.tasks.as_ref().map(|tasks| !tasks.is_empty()),
            !wired,
        ),
        ("reason", Some(!subsystem.reason.trim().is_empty()), true),
    ];
    fields
        .into_iter()
        .filter_map(|(field, filled, required)| {
            let wrong = if required {
                filled != Some(true)
            } else {
                filled.is_some()
            };
            wrong.then(|| Violation::SubsystemField {
                subsystem: name.to_owned(),
                field,
                status: subsystem.status.as_str(),
                required,
            })
        })
        .collect()
}

fn wired(
    name: &str,
    entry: &str,
    krate: &str,
    consumer: &str,
    index: &Index<'_>,
) -> Vec<Violation> {
    let unknown = |field, package: &str| Violation::SubsystemUnknownCrate {
        subsystem: name.to_owned(),
        field,
        krate: package.to_owned(),
    };
    let (Some(defining), Some(consuming)) = (index.product.get(krate), index.product.get(consumer))
    else {
        let mut violations = Vec::new();
        if !index.product.contains_key(krate) {
            violations.push(unknown("crate", krate));
        }
        if !index.product.contains_key(consumer) {
            violations.push(unknown("consumer", consumer));
        }
        return violations;
    };
    if !rust_items::definitions(&defining.code).contains(entry) {
        return vec![Violation::SubsystemEntryNotDefined {
            subsystem: name.to_owned(),
            entry: entry.to_owned(),
            krate: krate.to_owned(),
        }];
    }
    let used = if krate == consumer {
        if !defining.sources.is_composition_root {
            return vec![Violation::SubsystemSelfConsumer {
                subsystem: name.to_owned(),
                krate: krate.to_owned(),
            }];
        }
        let binary = defining
            .code
            .iter()
            .filter(|file| is_listed(&file.path, &defining.sources.binary_files));
        rust_items::references(binary, entry)
    } else {
        let mut violations = Vec::new();
        if rust_items::definitions(&consuming.code).contains(entry) {
            violations.push(Violation::SubsystemShadowed {
                subsystem: name.to_owned(),
                entry: entry.to_owned(),
                consumer: consumer.to_owned(),
            });
        }
        if !consuming.sources.dependencies.contains(&krate) {
            violations.push(Violation::SubsystemConsumerLacksDependency {
                subsystem: name.to_owned(),
                consumer: consumer.to_owned(),
                krate: krate.to_owned(),
            });
        }
        if !violations.is_empty() {
            return violations;
        }
        rust_items::references(&consuming.code, entry)
    };
    if used {
        return Vec::new();
    }
    vec![Violation::SubsystemNotWired {
        subsystem: name.to_owned(),
        entry: entry.to_owned(),
        consumer: consumer.to_owned(),
    }]
}

fn is_listed(path: &Path, files: &[PathBuf]) -> bool {
    files.iter().any(|file| file == path)
}

fn planned(
    name: &str,
    entry: &str,
    subsystem: &Subsystem,
    plan: &PlanTasks,
    index: &Index<'_>,
) -> Vec<Violation> {
    let tasks = subsystem.tasks.iter().flatten().filter_map(|task| {
        plan.check(task)
            .err()
            .map(|problem| Violation::SubsystemTask {
                subsystem: name.to_owned(),
                task: task.clone(),
                problem,
            })
    });
    let mentions = index.all.iter().filter_map(|(path, tokens)| {
        rust_items::first_mention(tokens, entry).map(|line| Violation::SubsystemHalfWired {
            subsystem: name.to_owned(),
            entry: entry.to_owned(),
            path: path.clone(),
            line,
        })
    });
    tasks.chain(mentions).collect()
}

fn is_identifier(text: &str) -> bool {
    text.chars()
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '_')
        && text.chars().all(|c| c.is_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan_tasks::TaskProblem;

    const CRATES: &str = "\
        [crates.lib]\nlayer = 0\nworkspace-deps = []\ndeps = []\ndev-deps = []\nbuild-deps = []\n\
        [crates.app]\nlayer = 1\nworkspace-deps = []\ndeps = []\ndev-deps = []\nbuild-deps = []\n";

    const PLAN: &str = "| C-01 | scheduler |\n| ~~C-02~~ | done |\n";

    fn file(path: &str, is_test_code: bool, text: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from(path),
            is_test_code,
            text: text.to_owned(),
        }
    }

    /// Checks one subsystem, written as the body of its table, against `lib` and `app`, the
    /// composition root, which depends on `lib`.
    fn violations(subsystem: &str, lib: &[SourceFile], app: &[SourceFile]) -> Vec<Violation> {
        violations_with(subsystem, lib, app, &["lib"])
    }

    fn violations_with(
        subsystem: &str,
        lib: &[SourceFile],
        app: &[SourceFile],
        app_dependencies: &[&'static str],
    ) -> Vec<Violation> {
        let architecture =
            Architecture::parse(&format!("{CRATES}[subsystems.registry]\n{subsystem}")).unwrap();
        let binary = |files: &[SourceFile]| -> Vec<PathBuf> {
            files
                .iter()
                .filter(|file| file.path.to_string_lossy().contains("/src/bin/"))
                .map(|file| file.path.clone())
                .collect()
        };
        let crates = [
            CrateSources {
                name: "lib",
                is_composition_root: false,
                binary_files: binary(lib),
                dependencies: Vec::new(),
                files: lib,
            },
            CrateSources {
                name: "app",
                is_composition_root: true,
                binary_files: app
                    .iter()
                    .filter(|file| !file.is_test_code)
                    .map(|file| file.path.clone())
                    .collect(),
                dependencies: app_dependencies.to_vec(),
                files: app,
            },
        ];
        check(&architecture, &PlanTasks::parse(PLAN).unwrap(), &crates)
    }

    const WIRED: &str = "status = \"wired\"\nentry = \"Registry\"\ncrate = \"lib\"\n\
                         consumer = \"app\"\nreason = \"stores machines\"\n";

    fn defining_lib() -> Vec<SourceFile> {
        vec![file("lib/src/lib.rs", false, "pub struct Registry;")]
    }

    #[test]
    fn a_wired_subsystem_used_by_its_consumer_passes() {
        let app = [file(
            "app/src/main.rs",
            false,
            "fn main() { let _r = lib::Registry; }",
        )];
        assert_eq!(violations(WIRED, &defining_lib(), &app), vec![]);
    }

    #[test]
    fn a_wired_subsystem_its_consumer_uses_only_in_tests_comments_or_strings_is_not_wired() {
        let app = [
            file(
                "app/src/main.rs",
                false,
                "// Registry\nfn main() { let _s = \"Registry\"; }\n\
                 #[cfg(test)]\nmod tests { use lib::Registry; }",
            ),
            file("app/tests/cli.rs", true, "use lib::Registry;"),
        ];
        assert_eq!(
            violations(WIRED, &defining_lib(), &app),
            vec![Violation::SubsystemNotWired {
                subsystem: "registry".into(),
                entry: "Registry".into(),
                consumer: "app".into(),
            }]
        );
    }

    #[test]
    fn a_wired_entry_must_be_defined_outside_tests_in_its_crate() {
        let lib = [
            file(
                "lib/src/lib.rs",
                false,
                "#[cfg(test)]\nmod tests { struct Registry; }",
            ),
            file("lib/tests/it.rs", true, "struct Registry;"),
        ];
        let app = [file("app/src/main.rs", false, "fn main() { Registry; }")];
        assert_eq!(
            violations(WIRED, &lib, &app),
            vec![Violation::SubsystemEntryNotDefined {
                subsystem: "registry".into(),
                entry: "Registry".into(),
                krate: "lib".into(),
            }]
        );
    }

    #[test]
    fn a_consumer_that_shadows_the_entry_or_lacks_the_dependency_is_not_wired() {
        let shadowing = [file(
            "app/src/main.rs",
            false,
            "struct Registry;\nfn main() { let _r = Registry; }",
        )];
        assert_eq!(
            violations(WIRED, &defining_lib(), &shadowing),
            vec![Violation::SubsystemShadowed {
                subsystem: "registry".into(),
                entry: "Registry".into(),
                consumer: "app".into(),
            }]
        );
        let unrelated = [file("app/src/main.rs", false, "fn main() { Registry; }")];
        assert_eq!(
            violations_with(WIRED, &defining_lib(), &unrelated, &[]),
            vec![Violation::SubsystemConsumerLacksDependency {
                subsystem: "registry".into(),
                consumer: "app".into(),
                krate: "lib".into(),
            }]
        );
    }

    #[test]
    fn a_library_with_a_stray_binary_may_not_consume_what_it_defines() {
        let own = "status = \"wired\"\nentry = \"Registry\"\ncrate = \"lib\"\n\
                   consumer = \"lib\"\nreason = \"r\"\n";
        let lib = [
            file("lib/src/lib.rs", false, "pub struct Registry;"),
            file("lib/src/bin/tool.rs", false, "fn main() { lib::Registry; }"),
        ];
        assert_eq!(
            violations(own, &lib, &[]),
            vec![Violation::SubsystemSelfConsumer {
                subsystem: "registry".into(),
                krate: "lib".into(),
            }]
        );
    }

    #[test]
    fn the_composition_root_uses_what_it_defines_only_in_its_binary_targets() {
        let root = "status = \"wired\"\nentry = \"Registry\"\ncrate = \"app\"\n\
                    consumer = \"app\"\nreason = \"r\"\n";
        let architecture =
            Architecture::parse(&format!("{CRATES}[subsystems.registry]\n{root}")).unwrap();
        let files = [
            file(
                "app/src/lib.rs",
                false,
                "pub struct Registry;\nfn f(_: Registry) {}",
            ),
            file("app/src/main.rs", false, "fn main() {}"),
        ];
        let crates = [CrateSources {
            name: "app",
            is_composition_root: true,
            binary_files: vec![PathBuf::from("app/src/main.rs")],
            dependencies: Vec::new(),
            files: &files,
        }];
        assert_eq!(
            check(&architecture, &PlanTasks::parse(PLAN).unwrap(), &crates),
            vec![Violation::SubsystemNotWired {
                subsystem: "registry".into(),
                entry: "Registry".into(),
                consumer: "app".into(),
            }],
            "a use in the library half does not count"
        );
    }

    #[test]
    fn only_the_composition_root_may_consume_what_it_defines() {
        let own = "status = \"wired\"\nentry = \"Registry\"\ncrate = \"lib\"\n\
                   consumer = \"lib\"\nreason = \"r\"\n";
        let lib = [file(
            "lib/src/lib.rs",
            false,
            "pub struct Registry;\nfn f(_: Registry) {}",
        )];
        assert_eq!(
            violations(own, &lib, &[]),
            vec![Violation::SubsystemSelfConsumer {
                subsystem: "registry".into(),
                krate: "lib".into(),
            }]
        );
        let root = "status = \"wired\"\nentry = \"Registry\"\ncrate = \"app\"\n\
                    consumer = \"app\"\nreason = \"r\"\n";
        let defined_only = [file(
            "app/src/main.rs",
            false,
            "struct Registry;\nfn main() {}",
        )];
        assert_eq!(
            violations(root, &[], &defined_only),
            vec![Violation::SubsystemNotWired {
                subsystem: "registry".into(),
                entry: "Registry".into(),
                consumer: "app".into(),
            }],
            "the definition itself is not a use"
        );
        let used = [file(
            "app/src/main.rs",
            false,
            "struct Registry;\nfn main() { Registry; }",
        )];
        assert_eq!(violations(root, &[], &used), vec![]);
    }

    #[test]
    fn wired_subsystems_name_workspace_crates() {
        let ghost = "status = \"wired\"\nentry = \"Registry\"\ncrate = \"ghost\"\n\
                     consumer = \"phantom\"\nreason = \"r\"\n";
        assert_eq!(
            violations(ghost, &[], &[]),
            vec![
                Violation::SubsystemUnknownCrate {
                    subsystem: "registry".into(),
                    field: "crate",
                    krate: "ghost".into(),
                },
                Violation::SubsystemUnknownCrate {
                    subsystem: "registry".into(),
                    field: "consumer",
                    krate: "phantom".into(),
                },
            ]
        );
    }

    #[test]
    fn a_planned_subsystem_has_open_tasks_and_no_code() {
        let planned = "status = \"planned\"\nentry = \"Registry\"\ntasks = [\"M1-C-01\"]\n\
                       reason = \"r\"\n";
        let mentioned_in_prose = [file("lib/src/lib.rs", false, "//! Registry comes later.")];
        assert_eq!(violations(planned, &mentioned_in_prose, &[]), vec![]);

        let half_wired = [file(
            "app/tests/cli.rs",
            true,
            "fn t() {}\nfn u(_: Registry) {}",
        )];
        assert_eq!(
            violations(planned, &[], &half_wired),
            vec![Violation::SubsystemHalfWired {
                subsystem: "registry".into(),
                entry: "Registry".into(),
                path: PathBuf::from("app/tests/cli.rs"),
                line: 2,
            }]
        );

        let bad_tasks = "status = \"planned\"\nentry = \"Registry\"\n\
                         tasks = [\"M1-C-02\", \"M1-C-99\", \"#4\"]\nreason = \"r\"\n";
        let problems: Vec<TaskProblem> = violations(bad_tasks, &[], &[])
            .into_iter()
            .filter_map(|violation| {
                if let Violation::SubsystemTask { problem, .. } = violation {
                    Some(problem)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            problems,
            [
                TaskProblem::Done,
                TaskProblem::Unknown,
                TaskProblem::Malformed
            ]
        );
    }

    #[test]
    fn each_status_requires_and_forbids_its_fields() {
        let fields = |subsystem: &str| -> Vec<(&'static str, bool)> {
            violations(subsystem, &defining_lib(), &[])
                .into_iter()
                .filter_map(|violation| {
                    if let Violation::SubsystemField {
                        field, required, ..
                    } = violation
                    {
                        Some((field, required))
                    } else {
                        None
                    }
                })
                .collect()
        };
        assert_eq!(
            fields(
                "status = \"wired\"\nentry = \"Registry\"\ntasks = [\"M1-C-01\"]\nreason = \" \"\n"
            ),
            [
                ("crate", true),
                ("consumer", true),
                ("tasks", false),
                ("reason", true)
            ]
        );
        assert_eq!(
            fields(
                "status = \"planned\"\nentry = \"Other\"\ncrate = \"lib\"\nconsumer = \"app\"\n\
                 tasks = []\nreason = \"r\"\n"
            ),
            [("crate", false), ("consumer", false), ("tasks", true)]
        );
    }

    #[test]
    fn the_entry_is_one_identifier() {
        for entry in ["lib::Registry", "Registry Store", "", "9lives"] {
            let subsystem = format!(
                "status = \"planned\"\nentry = \"{entry}\"\ntasks = [\"M1-C-01\"]\nreason = \"r\"\n"
            );
            assert_eq!(
                violations(&subsystem, &[], &[]),
                vec![Violation::SubsystemEntryNotIdentifier {
                    subsystem: "registry".into(),
                    entry: entry.into(),
                }],
                "{entry}"
            );
        }
    }

    #[test]
    fn no_subsystems_means_nothing_to_check() {
        let architecture = Architecture::parse(CRATES).unwrap();
        assert_eq!(check(&architecture, &PlanTasks::default(), &[]), vec![]);
    }
}
