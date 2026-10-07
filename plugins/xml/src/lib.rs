//! XML analysis configuration is independent plugin policy; the host owns transport and documents.
mod completion;
mod configuration;
mod structure;
use plugin_protocol::{
    api,
    bindings::{Guest, export},
};

struct XmlLanguage;
impl Guest for XmlLanguage {
    /// Preparation is stateless; later language features can share this dispatch without owning text.
    fn dispatch(payload: String) -> Result<String, String> {
        api::guest::dispatch(&payload, |input| match input {
            api::Input::Event {
                event: api::Notification::LanguageService(context),
                ..
            } => Ok(api::Output {
                language_service: Some(configuration::prepare(context)?),
                ..Default::default()
            }),
            api::Input::Event {
                event: api::Notification::LanguageCompletion(request),
                ..
            } => Ok(api::Output {
                language_completion: Some(completion::complete(request)),
                ..Default::default()
            }),
            api::Input::Snapshot => Ok(api::Output {
                snapshot: Some(Default::default()),
                ..Default::default()
            }),
            api::Input::Event {
                event: api::Notification::LanguageStructure(request),
                ..
            } => Ok(api::Output {
                language_structure: Some(structure::describe(request)),
                ..Default::default()
            }),
            _ => Ok(Default::default()),
        })
    }
}
export!(XmlLanguage);
