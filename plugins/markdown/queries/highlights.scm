; Block captures use the upstream Markdown node schema and the editor's established theme scopes.
(atx_heading (inline) @title)
(setext_heading (paragraph) @title)
[(atx_h1_marker) (atx_h2_marker) (atx_h3_marker) (atx_h4_marker)
 (atx_h5_marker) (atx_h6_marker) (setext_h1_underline) (setext_h2_underline)] @punctuation.special
[(indented_code_block) (fenced_code_block) (link_title)] @text.literal
(fenced_code_block_delimiter) @punctuation.delimiter
(link_destination) @link_uri
(link_label) @link_text
[(list_marker_plus) (list_marker_minus) (list_marker_star) (list_marker_dot)
 (list_marker_parenthesis) (thematic_break) (block_quote_marker) (block_continuation)] @punctuation.special
(backslash_escape) @string.escape
