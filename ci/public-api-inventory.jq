def public_path_items:
  .paths
  | to_entries[]
  | select(.value.crate_id == 0)
  | select(.value.kind == "module"
      or .value.kind == "struct"
      or .value.kind == "enum"
      or .value.kind == "function"
      or .value.kind == "constant"
      or .value.kind == "variant")
  | "\(.value.kind) \(.value.path | join("::"))";


def public_struct_fields($root):
  $root.paths
  | to_entries[]
  | select(.value.crate_id == 0 and .value.kind == "struct")
  | .key as $id
  | (.value.path | join("::")) as $path
  | ($root.index[$id].inner.struct.kind.plain.fields // [])[] as $field_id
  | $root.index[($field_id | tostring)]
  | select(.visibility == "public")
  | "field \($path).\(.name)";

def public_variant_fields($root):
  $root.paths
  | to_entries[]
  | select(.value.crate_id == 0 and .value.kind == "variant")
  | .key as $id
  | (.value.path | join("::")) as $path
  | $root.index[$id].inner.variant.kind as $kind
  | if ($kind | type) == "object" then
      (($kind.struct.fields // $kind.tuple // []) | to_entries[])
    else
      empty
    end as $field
  | select($field.value != null)
  | ($root.index[($field.value | tostring)].name // ($field.key | tostring)) as $name
  | "field \($path).\($name)";

def public_inherent_items($root):
  $root.paths
  | to_entries[]
  | select(.value.crate_id == 0)
  | select(.value.kind == "struct" or .value.kind == "enum")
  | {id: .key, path: (.value.path | join("::")), kind: .value.kind} as $type
  | (if $type.kind == "struct" then
       ($root.index[$type.id].inner.struct.impls // [])[]
     else
       ($root.index[$type.id].inner.enum.impls // [])[]
     end) as $impl_id
  | $root.index[($impl_id | tostring)].inner.impl as $impl
  | select($impl.trait == null)
  | $impl.items[] as $item_id
  | $root.index[($item_id | tostring)]
  | select(.visibility == "public")
  | (.inner | keys[0]) as $kind
  | select($kind == "function" or $kind == "assoc_const" or $kind == "assoc_type")
  | (if $kind == "assoc_const" then "constant"
     elif $kind == "assoc_type" then "type"
     else "function"
     end) as $label
  | "\($label) \($type.path)::\(.name)";

def explicit_trait_impls($root):
  $root.paths
  | to_entries[]
  | select(.value.crate_id == 0)
  | select(.value.kind == "struct" or .value.kind == "enum")
  | {id: .key, path: (.value.path | join("::")), kind: .value.kind} as $type
  | (if $type.kind == "struct" then
       ($root.index[$type.id].inner.struct.impls // [])[]
     else
       ($root.index[$type.id].inner.enum.impls // [])[]
     end) as $impl_id
  | $root.index[($impl_id | tostring)].inner.impl as $impl
  | select($impl.trait != null
      and $impl.is_synthetic == false
      and $impl.blanket_impl == null
      and $impl.trait.path != "StructuralPartialEq")
  | "impl \($impl.trait.path) for \($type.path)";

def extensibility_attributes($root):
  $root.paths
  | to_entries[]
  | select(.value.crate_id == 0)
  | select(.value.kind == "struct" or .value.kind == "enum")
  | .key as $id
  | (.value.path | join("::")) as $path
  | $root.index[$id].attrs[]?
  | select(. == "non_exhaustive")
  | "attribute #[non_exhaustive] \($path)";

. as $root
| [
    public_path_items,
    public_struct_fields($root),
    public_variant_fields($root),
    public_inherent_items($root),
    explicit_trait_impls($root),
    extensibility_attributes($root)
  ]
| unique
| sort[]
