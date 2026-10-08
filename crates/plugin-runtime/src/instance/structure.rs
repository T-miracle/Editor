//! A typed structure invocation shares the pure worker sandbox, without sharing another hook's result channel.
use super::*;
use serde::Deserialize as _;

/// Structure's 128 node levels each contain an object and children array, plus a small ABI envelope.
const MAX_JSON_DEPTH: usize = 272;
/// Proposal validation keeps its 512 KiB quota; this allows only the enclosing completion fields.
const MAX_COMPLETION_BYTES: usize = 512 * 1024 + 4096;

/// Decode only a typed structure callback after checking its raw size and container nesting without recursion.
/// Other capabilities retain serde_json's default depth limit. Typed validation still rejects illegal proposals.
pub(super) fn decode_completion(payload: &str) -> anyhow::Result<api::Completion> {
    anyhow::ensure!(
        payload.len() <= MAX_COMPLETION_BYTES,
        "Structure completion exceeds its encoded byte quota"
    );
    let mut containers = [0_u8; MAX_JSON_DEPTH];
    let mut depth = 0;
    let mut quoted = false;
    let mut escaped = false;
    for byte in payload.bytes() {
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            continue;
        }
        match byte {
            b'"' => quoted = true,
            b'{' | b'[' => {
                anyhow::ensure!(
                    depth < MAX_JSON_DEPTH,
                    "Structure JSON nesting is excessive"
                );
                containers[depth] = byte;
                depth += 1;
            }
            b'}' | b']' => {
                anyhow::ensure!(depth != 0, "Unbalanced structure JSON container");
                depth -= 1;
                anyhow::ensure!(
                    containers[depth] == if byte == b'}' { b'{' } else { b'[' },
                    "Mismatched structure JSON container"
                );
            }
            _ => {}
        }
    }
    anyhow::ensure!(!quoted && depth == 0, "Unterminated structure JSON value");
    // Only the bounded, string-aware scan above authorizes lifting serde's smaller generic JSON limit.
    let mut decoder = serde_json::Deserializer::from_str(payload);
    decoder.disable_recursion_limit();
    let completion = api::Completion::deserialize(&mut decoder)?;
    decoder.end()?;
    Ok(completion)
}

impl Instance {
    /// Consume one immutable source; pure initialization and other callbacks cannot publish structure data.
    pub(crate) fn describe_snapshot(
        &mut self,
        request: plugin_protocol::structure::Request,
    ) -> anyhow::Result<api::Output> {
        anyhow::ensure!(
            self.store.data().language_pure,
            "Structure requires a pure language worker"
        );
        self.store.data_mut().language_structure_call = true;
        let result = self.notify(None, api::Notification::LanguageStructure(request));
        self.store.data_mut().language_structure_call = false;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Construct JSON directly so neither a recursive serializer nor the test harness supplies extra stack headroom.
    fn nested_completion(depth: usize) -> String {
        let label = serde_json::to_string("嵌套 {[ ]} \\\"🙂").unwrap();
        let node = format!(
            "{{\"name\":{label},\"kind\":\"definition\",\"range\":{{\"start\":0,\"end\":1}},\"definition\":{{\"start\":0,\"end\":1}},\"children\":["
        );
        let mut payload = String::from(
            "{\"id\":1,\"result\":{\"Ok\":{\"snapshot\":null,\"language_structure\":{\"request\":1,\"document\":{\"id\":\"deep\",\"path\":\"deep.xml\",\"revision\":1},\"nodes\":[",
        );
        payload.push_str(&node.repeat(depth));
        payload.push_str(&"]}".repeat(depth));
        payload.push_str("],\"folds\":[]}}}}");
        payload
    }

    /// The maximum public node depth must decode and drop on the production-sized stack, even in debug builds.
    #[test]
    fn maximum_structure_depth_decodes_and_drops_on_two_mib_stack() {
        std::thread::Builder::new()
            .name("bounded-structure-decoder".into())
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let payload = nested_completion(128);
                assert!(
                    serde_json::from_str::<api::Completion>(&payload).is_err(),
                    "the generic JSON limit is smaller than the accepted structure contract"
                );
                let completion = decode_completion(&payload).unwrap();
                let output = completion.result.as_ref().unwrap();
                let mut nodes = output.language_structure.as_ref().unwrap().nodes.as_slice();
                let mut count = 0;
                while let Some(node) = nodes.first() {
                    assert_eq!(nodes.len(), 1);
                    count += 1;
                    nodes = &node.children;
                }
                assert_eq!(count, 128);
                // Drop recurses through the typed tree too; it belongs inside the same constrained thread.
                drop(completion);
            })
            .unwrap()
            .join()
            .unwrap();
    }

    /// Quotas run before typed deserialization; balanced tails and malformed string escapes still fail the final parser.
    #[test]
    fn structure_decoder_rejects_oversize_overdepth_and_trailing_json() {
        let error = decode_completion(&nested_completion(140)).unwrap_err();
        assert!(error.to_string().contains("nesting"));
        let error = decode_completion(&" ".repeat(MAX_COMPLETION_BYTES + 1)).unwrap_err();
        assert!(error.to_string().contains("byte quota"));
        let valid = nested_completion(1);
        assert!(decode_completion(&(valid.clone() + " {} ")).is_err());
        assert!(decode_completion(&(valid.clone() + " false ")).is_err());
        assert!(decode_completion(&(valid.clone() + " ] ")).is_err());
        assert!(decode_completion(&valid[..valid.len() - 1]).is_err());
        assert!(decode_completion(&valid.replace("definition", "\\q")).is_err());
    }
}
