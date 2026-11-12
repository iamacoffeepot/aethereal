//! The containers: each lays its children out and draws nothing of its own, its style's background and border aside.

use crate::style::Style;
use crate::widget::{Arrange, Axis, Draws, Laid, Stateless, Widget};

/// Its children one after another across, by its style's flex settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row;

/// Its children one after another down, by its style's flex settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Column;

/// Its children in the cells of its style's columns and rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grid;

/// Its children each over the one before, in one cell its size: a later child draws over an earlier one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stack;

impl Widget for Row {
    type State = Stateless;

    fn arrange(&self) -> Arrange {
        Arrange::Flex(Axis::Row)
    }

    fn draw(&self, _: &Stateless, _: &Style, _: &Laid, _: &mut Draws<'_, '_>) {}
}

impl Widget for Column {
    type State = Stateless;

    fn arrange(&self) -> Arrange {
        Arrange::Flex(Axis::Column)
    }

    fn draw(&self, _: &Stateless, _: &Style, _: &Laid, _: &mut Draws<'_, '_>) {}
}

impl Widget for Grid {
    type State = Stateless;

    fn arrange(&self) -> Arrange {
        Arrange::Grid
    }

    fn draw(&self, _: &Stateless, _: &Style, _: &Laid, _: &mut Draws<'_, '_>) {}
}

impl Widget for Stack {
    type State = Stateless;

    fn arrange(&self) -> Arrange {
        Arrange::Stack
    }

    fn draw(&self, _: &Stateless, _: &Style, _: &Laid, _: &mut Draws<'_, '_>) {}
}
