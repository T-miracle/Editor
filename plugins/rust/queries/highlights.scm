; Rust syntax captures for declarations, names, literals, comments, and punctuation.

(type_identifier) @type
(primitive_type) @type.builtin
(field_identifier) @property

(function_item name: (identifier) @function)
(function_signature_item name: (identifier) @function)
(struct_item name: (type_identifier) @type)
(enum_item name: (type_identifier) @type)
(trait_item name: (type_identifier) @type)
(type_item name: (type_identifier) @type)
(mod_item name: (identifier) @namespace)

(call_expression function: (identifier) @function)
(call_expression function: (field_expression field: (field_identifier) @function.method))
(macro_invocation macro: (identifier) @function.macro)

(parameter pattern: (identifier) @variable.parameter)
(let_declaration pattern: (identifier) @variable)
(line_comment) @comment
(block_comment) @comment
(string_literal) @string
(raw_string_literal) @string
(char_literal) @string
(boolean_literal) @boolean
(integer_literal) @number
(float_literal) @number
(attribute_item) @attribute
(inner_attribute_item) @attribute

[
  "as" "async" "await" "break" "const" "continue" "dyn" "else" "enum"
  "extern" "fn" "for" "if" "impl" "in" "let" "loop" "match" "mod"
  "move" "pub" "ref" "return" "static" "struct"
  "trait" "type" "union" "unsafe" "use" "where" "while"
] @keyword

; `mut` and `self` are named Rust grammar nodes rather than anonymous tokens.
(mutable_specifier) @keyword
(self) @variable.builtin

[
  "(" ")" "[" "]" "{" "}"
] @punctuation.bracket

[
  "," "." ";" ":" "::"
] @punctuation.delimiter

[
  "=" "+" "-" "*" "/" "%" "!" "&" "|" "^" "<" ">" "?"
] @operator
