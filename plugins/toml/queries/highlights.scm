; TOML keys and values are captured using the Tree-sitter TOML node names.
(bare_key) @type
(quoted_key) @string

(pair
  (bare_key)) @property

(pair
  (dotted_key
    (bare_key) @property))

; Scalar values and comments receive semantic highlight captures.
(boolean) @boolean
(comment) @comment
(string) @string

[
  (integer)
  (float)
] @number

[
  (offset_date_time)
  (local_date_time)
  (local_date)
  (local_time)
] @string.special

; TOML assignments and collection delimiters use the common editor captures.
[
  "."
  ","
] @punctuation.delimiter

"=" @operator

[
  "["
  "]"
  "[["
  "]]"
  "{"
  "}"
] @punctuation.bracket
