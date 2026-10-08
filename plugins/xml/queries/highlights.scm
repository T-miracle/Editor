; The upstream XML grammar separates tag names, attribute names and quoted values.
(STag (Name) @tag)
(ETag (Name) @tag)
(EmptyElemTag (Name) @tag)
(Attribute (Name) @attribute)
(Attribute (AttValue) @string)
(Comment) @comment
(EntityRef) @string.special
(CharRef) @string.special
(CDSect (CData) @string)
(PI (PITarget) @keyword)
(doctypedecl "DOCTYPE" @keyword)
(doctypedecl (Name) @type)
(SystemLiteral (URI) @string)
(PubidLiteral) @string
; XML declarations and DTD rules remain readable inside ordinary XML files.
"xml" @keyword
["version" "encoding" "standalone"] @attribute
(VersionNum) @number
(EncName) @string
["PUBLIC" "SYSTEM" "ELEMENT" "ATTLIST" "ENTITY" "NOTATION"] @keyword
["<" ">" "</" "/>" "<?" "?>" "<!" "]]>"] @punctuation.bracket
"=" @operator
