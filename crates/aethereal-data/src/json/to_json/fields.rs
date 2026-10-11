//! Struct and enum, by layout, for the bytes-to-JSON walk.

use super::Walk;
use crate::json::escape;
use crate::json::layout::{At, Layout};
use crate::plain::read_selector;
use crate::{FieldNode, JsonError, JsonFault, JsonStep, VariantNode};

impl Walk<'_, '_> {
    /// Writes a struct's fields by layout.
    pub(super) fn struct_value(&mut self, at: At<'_>, fields: &[FieldNode]) -> Result<(), JsonError> {
        let layout = Layout::of_struct(fields);
        match layout {
            Layout::Empty => {
                self.out.push_str("{}");
                Ok(())
            }
            Layout::Single(index) => {
                let child = at.child(index)?;
                self.value(child)
            }
            Layout::Several(indices) => {
                self.out.push('[');
                for (position, index) in indices.iter().enumerate() {
                    let first = position == 0;
                    if !first {
                        self.out.push(',');
                    }
                    let field_index = *index;
                    let child = at.child(field_index)?;
                    self.value(child).map_err(|error| error.within(JsonStep::Index(position)))?;
                }
                self.out.push(']');
                Ok(())
            }
            Layout::Named(named) => {
                self.out.push('{');
                for (position, field) in named.iter().enumerate() {
                    let first = position == 0;
                    if !first {
                        self.out.push(',');
                    }
                    escape::write(&field.name, &mut self.out);
                    self.out.push(':');
                    let child = at.child(field.shape)?;
                    self.value(child).map_err(|error| error.within(JsonStep::Field(field.name.clone())))?;
                }
                self.out.push('}');
                Ok(())
            }
        }
    }

    /// Writes an enum's selector and its variant's fields by layout.
    pub(super) fn enum_value(&mut self, at: At<'_>, variants: &[VariantNode]) -> Result<(), JsonError> {
        let selector = read_selector(&mut self.input)?;
        for variant in variants {
            let matches = variant.selector == selector;
            if !matches {
                continue;
            }
            let layout = Layout::of_variant(&variant.fields);
            match layout {
                Layout::Empty => {
                    escape::write(&variant.name, &mut self.out);
                    return Ok(());
                }
                Layout::Single(index) => {
                    self.out.push('{');
                    escape::write(&variant.name, &mut self.out);
                    self.out.push(':');
                    let child = at.child(index)?;
                    self.value(child).map_err(|error| error.within(JsonStep::Variant(variant.name.clone())))?;
                    self.out.push('}');
                    return Ok(());
                }
                Layout::Several(indices) => {
                    self.out.push('{');
                    escape::write(&variant.name, &mut self.out);
                    self.out.push(':');
                    self.out.push('[');
                    for (position, index) in indices.iter().enumerate() {
                        let first = position == 0;
                        if !first {
                            self.out.push(',');
                        }
                        let field_index = *index;
                        let child = at.child(field_index)?;
                        self.value(child)
                            .map_err(|error| error.within(JsonStep::Index(position)))
                            .map_err(|error| error.within(JsonStep::Variant(variant.name.clone())))?;
                    }
                    self.out.push(']');
                    self.out.push('}');
                    return Ok(());
                }
                Layout::Named(named) => {
                    self.out.push('{');
                    escape::write(&variant.name, &mut self.out);
                    self.out.push(':');
                    self.out.push('{');
                    for (position, field) in named.iter().enumerate() {
                        let first = position == 0;
                        if !first {
                            self.out.push(',');
                        }
                        escape::write(&field.name, &mut self.out);
                        self.out.push(':');
                        let child = at.child(field.shape)?;
                        self.value(child)
                            .map_err(|error| error.within(JsonStep::Field(field.name.clone())))
                            .map_err(|error| error.within(JsonStep::Variant(variant.name.clone())))?;
                    }
                    self.out.push('}');
                    self.out.push('}');
                    return Ok(());
                }
            }
        }

        Err(JsonFault::UnknownSelector(selector).into())
    }
}
