//! Struct and enum, by layout, for the tree-to-bytes walk.

use super::Walk;
use crate::json::layout::{At, Layout};
use crate::json::tree::Value;
use crate::plain::write_selector;
use crate::{FieldNode, SchemaNode, VariantNode};
use crate::{JsonError, JsonFault, JsonForm, JsonStep};
use alloc::string::String;
use alloc::vec::Vec;

impl Walk<'_, '_> {
    /// Reads a struct's fields by layout.
    pub(super) fn struct_value(
        &self,
        at: At<'_>,
        fields: &[FieldNode],
        value: &Value<'_>,
        out: &mut Vec<u8>,
    ) -> Result<(), JsonError> {
        let layout = Layout::of_struct(fields);
        match layout {
            Layout::Empty => self.empty_object(value),
            Layout::Single(index) => {
                let child = at.child(index)?;
                self.value(child, value, out)
            }
            Layout::Several(indices) => {
                let child_at = at;
                self.positional_fields(child_at, &indices, value, out)
            }
            Layout::Named(named) => self.named_fields(at, named, value, out),
        }
    }

    /// Reads an enum's selector and its variant's fields by layout.
    pub(super) fn enum_value(
        &self,
        at: At<'_>,
        variants: &[VariantNode],
        value: &Value<'_>,
        out: &mut Vec<u8>,
    ) -> Result<(), JsonError> {
        match value {
            Value::String(name) => {
                let found = variants.iter().find(|variant| variant.name == *name);
                let Some(variant) = found else {
                    let refused: JsonError = JsonFault::UnknownVariant.into();
                    return Err(refused.within(JsonStep::Variant(name.clone())));
                };
                let layout = Layout::of_variant(&variant.fields);
                let is_empty = matches!(layout, Layout::Empty);
                if !is_empty {
                    let refused: JsonError = JsonFault::Expected(JsonForm::Object).into();
                    return Err(refused.within(JsonStep::Variant(name.clone())));
                }
                Self::selector(variant.selector, out);
                Ok(())
            }
            Value::Object(span) => {
                let span_value = *span;
                let members = self.tree.members(span_value);
                let single = members.len() == 1;
                if !single {
                    return Err(JsonFault::Expected(JsonForm::Variant).into());
                }
                let Some(member) = members.first() else {
                    return Err(JsonFault::Expected(JsonForm::Variant).into());
                };
                let found = variants.iter().find(|variant| variant.name == member.name);
                let Some(variant) = found else {
                    let refused: JsonError = JsonFault::UnknownVariant.into();
                    return Err(refused.within(JsonStep::Variant(member.name.clone())));
                };
                let layout = Layout::of_variant(&variant.fields);
                match layout {
                    Layout::Empty => {
                        let refused: JsonError = JsonFault::Expected(JsonForm::String).into();
                        Err(refused.within(JsonStep::Variant(member.name.clone())))
                    }
                    Layout::Single(index) => {
                        let child = at.child(index)?;
                        Self::selector(variant.selector, out);
                        self.value(child, &member.value, out)
                            .map_err(|error| error.within(JsonStep::Variant(member.name.clone())))
                    }
                    Layout::Several(indices) => {
                        Self::selector(variant.selector, out);
                        self.variant_positional(at, &indices, &member.value, &member.name, out)
                    }
                    Layout::Named(named) => {
                        Self::selector(variant.selector, out);
                        self.named_fields(at, named, &member.value, out)
                            .map_err(|error| error.within(JsonStep::Variant(member.name.clone())))
                    }
                }
            }
            Value::Null | Value::Boolean(_) | Value::Number(_) | Value::Array(_) => {
                Err(JsonFault::Expected(JsonForm::Variant).into())
            }
        }
    }

    /// Reads an empty object with no members.
    fn empty_object(&self, value: &Value<'_>) -> Result<(), JsonError> {
        let Value::Object(span) = value else {
            return Err(JsonFault::Expected(JsonForm::Object).into());
        };
        let span_value = *span;
        let members = self.tree.members(span_value);
        let empty = members.is_empty();
        if empty {
            return Ok(());
        }
        let Some(first) = members.first() else {
            return Ok(());
        };
        let refused: JsonError = JsonFault::UnknownField.into();
        Err(refused.within(JsonStep::Field(first.name.clone())))
    }

    /// Reads several positional fields from an array.
    fn positional_fields(
        &self,
        at: At<'_>,
        indices: &[u32],
        value: &Value<'_>,
        out: &mut Vec<u8>,
    ) -> Result<(), JsonError> {
        let Value::Array(span) = value else {
            return Err(JsonFault::Expected(JsonForm::Array).into());
        };
        let span_value = *span;
        let elements = self.tree.elements(span_value);
        let expected = indices.len();
        let found = elements.len();
        let right_length = expected == found;
        if !right_length {
            return Err(JsonFault::WrongLength { expected, found }.into());
        }
        for (position, index) in indices.iter().enumerate() {
            let field_index = *index;
            let child = at.child(field_index)?;
            let Some(element) = elements.get(position) else {
                return Err(JsonFault::WrongLength { expected, found }.into());
            };
            self.value(child, element, out).map_err(|error| error.within(JsonStep::Index(position)))?;
        }
        Ok(())
    }

    /// Reads a variant's positional fields, adding the variant step outside each index.
    fn variant_positional(
        &self,
        at: At<'_>,
        indices: &[u32],
        value: &Value<'_>,
        name: &str,
        out: &mut Vec<u8>,
    ) -> Result<(), JsonError> {
        let Value::Array(span) = value else {
            let refused: JsonError = JsonFault::Expected(JsonForm::Array).into();
            return Err(refused.within(JsonStep::Variant(String::from(name))));
        };
        let span_value = *span;
        let elements = self.tree.elements(span_value);
        let expected = indices.len();
        let found = elements.len();
        let right_length = expected == found;
        if !right_length {
            let refused: JsonError = JsonFault::WrongLength { expected, found }.into();
            return Err(refused.within(JsonStep::Variant(String::from(name))));
        }
        for (position, index) in indices.iter().enumerate() {
            let field_index = *index;
            let child = at.child(field_index)?;
            let Some(element) = elements.get(position) else {
                let refused: JsonError = JsonFault::WrongLength { expected, found }.into();
                return Err(refused.within(JsonStep::Variant(String::from(name))));
            };
            self.value(child, element, out)
                .map_err(|error| error.within(JsonStep::Index(position)))
                .map_err(|error| error.within(JsonStep::Variant(String::from(name))))?;
        }
        Ok(())
    }

    /// Reads named fields from an object in declared order.
    fn named_fields(
        &self,
        at: At<'_>,
        fields: &[FieldNode],
        value: &Value<'_>,
        out: &mut Vec<u8>,
    ) -> Result<(), JsonError> {
        let Value::Object(span) = value else {
            return Err(JsonFault::Expected(JsonForm::Object).into());
        };
        let span_value = *span;
        let mut found_count = 0;
        for field in fields {
            let child = at.child(field.shape)?;
            let found = self.tree.member(span_value, &field.name);
            let Some(member_value) = found else {
                let is_option = matches!(child.node(), SchemaNode::Option(_));
                if is_option {
                    out.push(0);
                    continue;
                }
                let refused: JsonError = JsonFault::MissingField.into();
                return Err(refused.within(JsonStep::Field(field.name.clone())));
            };
            self.value(child, member_value, out).map_err(|error| error.within(JsonStep::Field(field.name.clone())))?;
            found_count += 1;
        }
        let members = self.tree.members(span_value);
        let has_extra = members.len() > found_count;
        if !has_extra {
            return Ok(());
        }
        for member in members {
            let mut is_field = false;
            for field in fields {
                let same = field.name == member.name;
                is_field = is_field || same;
            }
            if !is_field {
                let refused: JsonError = JsonFault::UnknownField.into();
                return Err(refused.within(JsonStep::Field(member.name.clone())));
            }
        }
        Ok(())
    }

    /// Appends one selector.
    fn selector(selector: u32, out: &mut Vec<u8>) {
        write_selector(out, selector);
    }
}
