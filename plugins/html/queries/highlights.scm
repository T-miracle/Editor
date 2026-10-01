; Markup names and declarations use the editor's standard semantic captures.
(tag_name) @tag
(erroneous_end_tag_name) @tag.error
(doctype) @constant
(attribute_name) @attribute

; Include quotes, unquoted values, character references, and HTML comments.
(attribute_value) @string
(quoted_attribute_value) @string
(entity) @string.special
(comment) @comment

; Markup punctuation stays separate from names and attribute values.
"=" @operator
[
  "<"
  ">"
  "</"
  "/>"
] @punctuation.bracket
