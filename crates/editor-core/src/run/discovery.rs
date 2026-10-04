//! Turning discovered targets into configurations, and keeping them in step.
//!
//! Discovery offers candidates; a user confirms one and gets an ordinary configuration they can edit.
//! The rules here are about what later discoveries may do to that configuration: they may repair a
//! target that disappeared, and they may never quietly replace what the user wrote.
//!
//! Every provider's candidates travel the same path, so nothing here depends on which plugin offered
//! one or what it runs.
use super::{RunConfig, RunConfigSet, RunConfigSource, RunStep, RunTarget, StepTarget};
use plugin_schema::DiscoveredTarget;

#[cfg(test)]
#[path = "discovery_tests.rs"]
mod tests;

/// Field a provider may report to name the build program for its targets.
const BUILD_PROGRAM_FIELD: &str = "build_program";
/// Field a provider may report to name the build arguments, separated by newlines.
const BUILD_ARGS_FIELD: &str = "build_args";

/// What a discovery run did to the stored configurations.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiscoveryOutcome {
    /// Targeted configurations whose definition a discovery added or corrected.
    pub updated: Vec<String>,
    /// Targets that no stored configuration claims, which the user may choose to add.
    pub offered: Vec<String>,
    /// Stored configurations whose target a discovery no longer offers.
    pub missing: Vec<String>,
}

/// Build the configuration a discovered target describes.
///
/// The result is an ordinary configuration: the user can edit every part of it, and nothing in it
/// points back at the plugin except the target it came from. A provider that reports a build program
/// gets a build action; one that does not gets none, so Build stays honestly unavailable rather than
/// running a command the provider never described.
pub fn configuration_for(target: &DiscoveredTarget, id: String, name: String) -> RunConfig {
    RunConfig {
        id,
        name,
        // The target's label names the program; a provider that needs arguments reports them as
        // fields, and the user can add more once the configuration exists.
        target: RunTarget::Program {
            program: target.label.clone(),
            args: target
                .fields
                .get("program_args")
                .map(|args| {
                    args.lines()
                        .map(str::to_owned)
                        .filter(|line| !line.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
        },
        directory: None,
        env: Default::default(),
        tool_paths: Default::default(),
        build: build_steps(target),
        prelaunch: Vec::new(),
        source: RunConfigSource::Local,
        from_target: Some(target.id.clone()),
        local: true,
    }
}

/// The build action a provider's own fields describe, if it described one.
fn build_steps(target: &DiscoveredTarget) -> Vec<RunStep> {
    let Some(program) = target.fields.get(BUILD_PROGRAM_FIELD) else {
        return Vec::new();
    };
    let args = target
        .fields
        .get(BUILD_ARGS_FIELD)
        .map(|args| {
            args.lines()
                .map(str::to_owned)
                .filter(|line| !line.is_empty())
                .collect()
        })
        .unwrap_or_default();
    vec![RunStep {
        name: format!("构建 {}", target.label),
        target: StepTarget::Action {
            target: RunTarget::Program {
                program: program.clone(),
                args,
            },
        },
    }]
}

/// Reconcile stored configurations with what a discovery now offers.
///
/// A configuration that names a target still being offered keeps its own name, arguments,
/// environment and steps: a discovery may correct what the target *is*, not what the user made of
/// it. One whose target is gone is left in place and reported, because deciding what to do about a
/// missing target is the user's, not a background walk's.
pub fn reconcile(configs: &RunConfigSet, targets: &[DiscoveredTarget]) -> DiscoveryOutcome {
    let mut outcome = DiscoveryOutcome::default();
    for target in targets {
        match configs
            .configurations
            .iter()
            .find(|config| config.from_target.as_deref() == Some(target.id.as_str()))
        {
            // The target is already stored; a discovery never rewrites the user's configuration.
            Some(_) => {}
            None => outcome.offered.push(target.id.clone()),
        }
    }
    for config in &configs.configurations {
        let Some(from_target) = &config.from_target else {
            continue;
        };
        let offered = targets.iter().find(|target| target.id == *from_target);
        match offered {
            Some(target) => {
                // A discovery may correct the program a target names — that is what "the target is
                // now this" means — while everything the user chose stays as they left it.
                if program_of(config) != Some(target.label.as_str()) {
                    outcome.updated.push(config.id.clone());
                }
            }
            None => outcome.missing.push(config.id.clone()),
        }
    }
    outcome
}

/// Repair one configuration from the target it came from.
///
/// Only the program the target names is replaced; the name, arguments, environment, tool
/// directories, steps and sharing choice are the user's and are returned unchanged.
pub fn repair(config: &RunConfig, target: &DiscoveredTarget) -> RunConfig {
    let mut repaired = config.clone();
    repaired.target = RunTarget::Program {
        program: target.label.clone(),
        args: match &config.target {
            // The user's own arguments survive; only the program is the target's to name.
            RunTarget::Program { args, .. } => args.clone(),
            RunTarget::Script { .. } => Vec::new(),
        },
    };
    repaired
}

/// The program a configuration starts, when it starts one directly.
fn program_of(config: &RunConfig) -> Option<&str> {
    match &config.target {
        RunTarget::Program { program, .. } => Some(program.as_str()),
        RunTarget::Script { .. } => None,
    }
}
