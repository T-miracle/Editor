//! Bounded asynchronous public configuration calls; the actor never parses tool-specific values.
use super::*;
use plugin_runtime::plugin_protocol::{api::RequestUpdate, configurations};

/// One catalog receipt retains its invocation identity and every unavailable contributor.
#[derive(Clone, Debug, Default)]
pub(crate) struct ConfigurationCatalog {
    pub templates: Vec<(String, configurations::Template)>,
    pub failures: BTreeMap<String, String>,
    pub origins: Vec<plugin_runtime::TargetOrigin>,
}

/// Replies are joined to host-generated request IDs before any window or launch consumes them.
pub(crate) struct ConfigurationReply {
    pub request: u64,
    pub origin: Option<plugin_runtime::TargetOrigin>,
    pub result: Result<serde_json::Value, String>,
}

#[derive(Default)]
pub(in crate::extensions) struct ConfigurationCalls {
    calls: BTreeMap<u64, (String, String, plugin_runtime::TargetRequest)>,
    catalogs: BTreeMap<u64, Vec<(String, Result<plugin_runtime::TargetRequest, String>)>>,
}

impl ConfigurationCalls {
    /// Dispatch only public signatures; existing runtime checks trust, grants and provider incarnation.
    pub fn dispatch(&mut self, work: Work, manager: &mut Manager, output: &Arc<Mutex<Published>>) {
        match work {
            Work::ConfigurationCatalog { request, arguments } => {
                if self.catalogs.len() >= 4 {
                    let mut catalog = ConfigurationCatalog::default();
                    catalog.failures.insert(
                        "catalog".into(),
                        "Configuration catalog quota exceeded".into(),
                    );
                    output
                        .lock()
                        .unwrap()
                        .configuration_catalogs
                        .push((request, catalog));
                    output.lock().unwrap().configuration_revision += 1;
                    return;
                }
                let calls = manager
                    .configuration_providers()
                    .into_iter()
                    .map(|provider| {
                        let call = manager
                            .begin_configuration_call(&provider, "catalog", arguments.clone())
                            .map_err(|error| format!("{error:#}"));
                        (provider, call)
                    })
                    .collect();
                self.catalogs.insert(request, calls);
            }
            Work::ConfigurationCall {
                request,
                provider,
                method,
                arguments,
            } => {
                let call = if self.calls.len() >= 128 {
                    Err("Configuration request quota exceeded".into())
                } else {
                    manager
                        .begin_configuration_call(&provider, &method, arguments)
                        .map_err(|error| format!("{error:#}"))
                };
                match call {
                    Ok(call) => {
                        self.calls.insert(request, (provider, method, call));
                    }
                    Err(error) => {
                        output
                            .lock()
                            .unwrap()
                            .configuration_replies
                            .push(ConfigurationReply {
                                request,
                                origin: None,
                                result: Err(error),
                            });
                        output.lock().unwrap().configuration_revision += 1;
                    }
                }
            }
            Work::CancelConfigurations { requests } => {
                for request in requests {
                    self.calls.remove(&request);
                    self.catalogs.remove(&request);
                }
            }
            _ => unreachable!("configuration dispatcher only receives configuration work"),
        }
    }

    /// Polling consumes each terminal receipt once and rejects retired providers before publication.
    pub fn poll(&mut self, manager: &Manager, output: &Arc<Mutex<Published>>) {
        let origins = manager.configuration_origins();
        {
            let mut published = output.lock().unwrap();
            if published.configuration_origins != origins {
                published.configuration_origins = origins;
                published.configuration_revision += 1;
            }
        }
        self.calls.retain(|request, (provider, method, call)| {
            let Some(mut result) = outcome(call, manager) else {
                return true;
            };
            if method == "form" {
                // Embedded forms receive the same negotiated native capabilities as plugin panels.
                result = result.and_then(|value| {
                    let form: configurations::Form = configurations::decode(value.clone())?;
                    manager
                        .validate_native_document(provider, &form.document)
                        .map_err(|error| format!("{error:#}"))?;
                    Ok(value)
                });
            }
            output
                .lock()
                .unwrap()
                .configuration_replies
                .push(ConfigurationReply {
                    request: *request,
                    origin: Some(call.origin()),
                    result,
                });
            output.lock().unwrap().configuration_revision += 1;
            false
        });
        self.catalogs.retain(|request, pending| {
            if pending.iter().any(|(_, call)| {
                call.as_ref()
                    .is_ok_and(|call| outcome(call, manager).is_none())
            }) {
                return true;
            }
            let mut catalog = ConfigurationCatalog::default();
            for (provider, call) in pending.iter() {
                let result = match call {
                    Ok(call) => outcome(call, manager).unwrap(),
                    Err(error) => Err(error.clone()),
                }
                .and_then(configurations::decode::<configurations::Catalog>);
                match result {
                    Ok(result)
                        if result.templates.len() <= 128
                            && result.templates.iter().all(template_valid)
                            && result
                                .templates
                                .iter()
                                .map(|template| &template.id)
                                .collect::<std::collections::BTreeSet<_>>()
                                .len()
                                == result.templates.len() =>
                    {
                        if catalog.templates.len() + result.templates.len() > 128 {
                            catalog
                                .failures
                                .insert(provider.clone(), "Template quota exceeded".into());
                            continue;
                        }
                        if let Ok(call) = call {
                            catalog.origins.push(call.origin());
                        }
                        catalog.templates.extend(
                            result
                                .templates
                                .into_iter()
                                .map(|template| (provider.clone(), template)),
                        );
                    }
                    Ok(_) => {
                        catalog
                            .failures
                            .insert(provider.clone(), "Invalid template catalog".into());
                    }
                    Err(error) => {
                        catalog.failures.insert(provider.clone(), error);
                    }
                }
            }
            output
                .lock()
                .unwrap()
                .configuration_catalogs
                .push((*request, catalog));
            output.lock().unwrap().configuration_revision += 1;
            false
        });
    }
}

/// Identity, presentation and opaque defaults all have generic bounds before entering native UI.
fn template_valid(template: &configurations::Template) -> bool {
    !template.id.is_empty()
        && template.id.len() <= 256
        && template.group.len() <= 256
        && !template.label.is_empty()
        && template.label.len() <= 256
        && template.icon.len() <= 16 * 1024
        // Native SVG icons contain no external assets or executable foreign content.
        && icon_valid(&template.icon)
        && template.defaults.len() <= configurations::MAX_VALUES_BYTES
        && serde_json::from_str::<serde_json::Value>(&template.defaults).is_ok()
}

/// Parse XML before handing artwork to the icon renderer; whitespace cannot hide resource links.
fn icon_valid(source: &str) -> bool {
    if !source.trim_start().starts_with('<') {
        return true;
    }
    let Ok(xml) = resvg::usvg::roxmltree::Document::parse(source) else {
        return false;
    };
    xml.root_element().tag_name().name() == "svg"
        && xml.descendants().count() <= 512
        && xml
            .descendants()
            .filter(|node| node.is_element())
            .all(|node| {
                !["script", "foreignObject", "image", "style", "use"]
                    .contains(&node.tag_name().name())
                    && node.attributes().all(|attribute| {
                        attribute.name() != "href"
                            && !attribute.name().starts_with("on")
                            && !attribute.value().to_ascii_lowercase().contains("url(")
                    })
            })
}

#[cfg(test)]
mod tests {
    /// Attribute spacing and namespaces do not bypass the self-contained artwork boundary.
    #[test]
    fn configuration_icons_reject_external_resources() {
        assert!(super::icon_valid(" <svg><path d='M0 0L1 1'/></svg>"));
        assert!(!super::icon_valid(
            "<svg><use href = 'https://example.com/a'/></svg>"
        ));
        assert!(!super::icon_valid("<svg><script>anything</script></svg>"));
        assert!(!super::icon_valid(
            "<svg><path fill='url(https://example.com)'/></svg>"
        ));
    }
}

fn outcome(
    call: &plugin_runtime::TargetRequest,
    manager: &Manager,
) -> Option<Result<serde_json::Value, String>> {
    if !call.valid_for(manager) {
        call.stop();
    }
    match call.status() {
        RequestUpdate::Accepted | RequestUpdate::Progress { .. } => None,
        RequestUpdate::Completed { result } if call.valid_for(manager) => {
            Some(result.map_err(|error| error.message))
        }
        RequestUpdate::Cancelled {
            reason: plugin_runtime::plugin_protocol::api::ErrorCode::TimedOut,
            ..
        } => Some(Err(rust_i18n::t!("run.plugin_timeout").into())),
        _ => Some(Err("Configuration provider or workspace retired".into())),
    }
}
