(class_declaration
  (identifier) @name) @definition.class

(function_declaration
  (identifier) @name) @definition.function

(object_declaration
  (identifier) @name) @definition.class

(property_declaration
  (variable_declaration
    (identifier) @name)) @definition.property
