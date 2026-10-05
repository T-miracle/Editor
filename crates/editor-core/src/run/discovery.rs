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
/// Field a provider may report to name the program's own arguments, separated by newlines.
const PROGRAM_ARGS_FIELD: &str = "program_args";

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
    let program = arguments(&target.fields, PROGRAM_ARGS_FIELD);
    let build = build_steps(target);
    RunConfig {
        id,
        name,
        // The program is the one the provider named, never its display label: a configuration that
        // ran a label would fail at launch with a message about a missing file.
        target: provided_target(target).unwrap_or_else(|| RunTarget::Program {
            program: target.program.clone(),
            args: program.clone(),
        }),
        directory: None,
        env: Default::default(),
        tool_paths: Default::default(),
        build,
        // Launch planning already executes the configuration's own build. A duplicated final
        // command would run twice or block forever, and a self build reference would be invalid.
        prelaunch: Vec::new(),
        source: RunConfigSource::Local,
        from_target: Some(target.id.clone()),
        // A discovered configuration follows the scope's provider choice until the user changes it.
        provider: None,
        breakpoints: Default::default(),
        local: true,
    }
}

/// One provider field as a literal argument list, one argument per line.
fn arguments(fields: &std::collections::BTreeMap<String, String>, key: &str) -> Vec<String> {
    fields
        .get(key)
        .map(|args| {
            args.lines()
                .map(str::to_owned)
                .filter(|line| !line.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// The build action a provider's own fields describe, if it described one.
fn build_steps(target: &DiscoveredTarget) -> Vec<RunStep> {
    if let Some(target) = provided_target(target) {
        return vec![RunStep {
            name: "Build target".into(),
            target: StepTarget::Action { target },
        }];
    }
    let Some(program) = target.fields.get(BUILD_PROGRAM_FIELD) else {
        return Vec::new();
    };
    let args = arguments(&target.fields, BUILD_ARGS_FIELD);
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

impl RunConfig {
    /// Match an explicitly confirmed discovery, including a portable provider binding reopened on
    /// another machine. This compares opaque bindings and never guesses language-specific IDs.
    pub fn claims_target(&self, target: &DiscoveredTarget) -> bool {
        self.from_target.as_deref() == Some(target.id.as_str())
            || matches!(&self.target,
            RunTarget::Provided {provider,binding,..} if provider==&target.provider && target.fields.get("provider_binding")==Some(binding))
    }
}

/// Reconcile stored configurations with what a discovery now offers.
///
/// A configuration that names a target still being offered keeps its own name, arguments,
/// environment and steps: a discovery reports what changed for the caller to confirm. One whose
/// target is gone is left in place and reported, because deciding what to do about a
/// missing target is the user's, not a background walk's.
pub fn reconcile(configs: &RunConfigSet, targets: &[DiscoveredTarget]) -> DiscoveryOutcome {
    let mut outcome = DiscoveryOutcome::default();
    for target in targets {
        match configs
            .configurations
            .iter()
            .find(|config| config.claims_target(target))
        {
            // The target is already stored; a discovery never rewrites the user's configuration.
            Some(_) => {}
            None => outcome.offered.push(target.id.clone()),
        }
    }
    for config in &configs.configurations {
        if config.from_target.is_none() && !matches!(config.target, RunTarget::Provided { .. }) {
            continue;
        }
        let offered = targets.iter().find(|target| config.claims_target(target));
        match offered {
            Some(target) => {
                // Report a proposal without applying it; only the user's repair action may replace
                // the stored executable, including one they have edited themselves.
                if program_of(config) != Some(target.program.as_str())
                    || matches!(&config.target,RunTarget::Provided {binding,..} if target.fields.get("provider_binding")!=Some(binding))
                {
                    outcome.updated.push(config.id.clone());
                }
            }
            None => outcome.missing.push(config.id.clone()),
        }
    }
    outcome
}

/// Prepare the repair the user explicitly chose for the target a configuration came from.
///
/// The confirmed target and its matching automatic build are replaced together. The user's name,
/// arguments, environment, tool directories, unrelated steps and sharing choice remain unchanged.
pub fn repair(config: &RunConfig, target: &DiscoveredTarget) -> RunConfig {
    let mut repaired = config.clone();
    let args = config.literal_arguments();
    let new_target = provided_target(target);
    // Retire only preparation that belongs to the old binding, including repairs to direct tools.
    // An unrelated provider action remains the user's independent ordered step.
    let mut replaced = false;
    repaired.build.retain_mut(|step| {
        let old_binding=matches!((&step.target,&config.target),
            (StepTarget::Action {target:RunTarget::Provided {provider:left,binding:a,..}},RunTarget::Provided {provider:right,binding:b,..}) if left==right&&a==b);
        if !old_binding {return true;}
        replaced=true;
        if let Some(target)=&new_target {
            step.target=StepTarget::Action {target:target.clone()};true
        } else {false}
    });
    if let Some(target) = &new_target
        && !replaced
    {
        repaired.build.insert(
            0,
            RunStep {
                name: "Build target".into(),
                target: StepTarget::Action {
                    target: target.clone(),
                },
            },
        );
    }
    repaired.target = new_target
        .map(|mut target| {
            if let RunTarget::Provided { args: values, .. } = &mut target {
                *values = args;
            }
            target
        })
        .unwrap_or_else(|| RunTarget::Program {
            program: target.program.clone(),
            args: match &config.target {
                // The user's own arguments survive; only the program is the target's to name.
                RunTarget::Program { args, .. } => args.clone(),
                RunTarget::Script { .. } => Vec::new(),
                RunTarget::Provided { args, .. } => args.clone(),
            },
        });
    repaired.from_target = Some(target.id.clone());
    repaired
}

/// The program a configuration starts, when it starts one directly.
fn program_of(config: &RunConfig) -> Option<&str> {
    match &config.target {
        RunTarget::Program { program, .. } => Some(program.as_str()),
        RunTarget::Script { .. } => None,
        RunTarget::Provided { label, .. } => Some(label),
    }
}

/// Reserved versioned fields carry a generic provider binding, never a language-specific command.
fn provided_target(target: &DiscoveredTarget) -> Option<RunTarget> {
    let binding = target.fields.get("provider_binding")?;
    Some(RunTarget::Provided {
        provider: target.provider.clone(),
        binding: binding.clone(),
        label: target.program.clone(),
        args: arguments(&target.fields, PROGRAM_ARGS_FIELD),
    })
}
