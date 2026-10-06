; Inline punctuation belongs to the inline parser; excluding anonymous children would remove emphasis delimiters.
; Parse separated prose ranges through the selected inline WASM provider without native fallback.
((inline) @injection.content
 (#set! injection.language "markdown_inline")
 (#set! injection.include-children)
 (#set! injection.combined))
