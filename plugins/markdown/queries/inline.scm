; Inline scopes are owned by this package, using the upstream v0.5.3 node schema.
(code_span) @text.code.span
((emphasis) @emphasis (#set! highlight.allow-overlap))
((strong_emphasis) @emphasis.strong (#set! highlight.allow-overlap))
; Muted styling distinguishes removed prose using an existing project theme scope.
(strikethrough) @comment
[(emphasis_delimiter) (code_span_delimiter)] @punctuation.delimiter
[(link_destination) (uri_autolink)] @link_uri
[(link_label) (link_text) (image_description)] @link_text
[(backslash_escape) (hard_line_break)] @string.escape
