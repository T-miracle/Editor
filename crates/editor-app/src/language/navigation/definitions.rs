//! Standard definition and type-definition requests share document synchronization and ownership.
use super::*;

impl LanguageServerConnection {
    /// Prefer a definition, then use the advertised type-definition target when no definition exists.
    ///
    /// Schema-backed languages may identify the constraint as a type definition. Both methods return
    /// ordinary locations; the host does not infer a target from the language name or source text.
    pub(super) fn definitions(
        &mut self,
        uri: Uri,
        source: String,
        position: Position,
    ) -> anyhow::Result<Vec<LocationLink>> {
        let uri_text = self.sync_document(uri, source)?;
        let params = json!({
            "textDocument": { "uri": uri_text },
            "position": position
        });
        if self.definition_provider {
            let response = self.request("textDocument/definition", params.clone())?;
            let locations = self.definition_locations(response)?;
            if !locations.is_empty() {
                return Ok(locations);
            }
        }
        if self.type_definition_provider {
            let response = self.request("textDocument/typeDefinition", params)?;
            return self.definition_locations(response);
        }
        Ok(Vec::new())
    }

    /// A relative Schema URI may retain the wire prefix; normalize only local-file targets.
    fn definition_locations(&self, response: Value) -> anyhow::Result<Vec<LocationLink>> {
        let mut locations = decode_definitions(response)?;
        for location in &mut locations {
            location.target_uri = self.logical_uri(&location.target_uri);
        }
        Ok(locations)
    }
}
