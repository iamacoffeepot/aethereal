//! A screen's pictures: one texture for each picture however many widgets show it, the layout drawn once it is made,
//! and released once no widget shows it, after the list that last named it is replaced.

use super::super::input::tests::laid;
use super::super::orders::Order;
use super::super::tests::{id, screen};
use super::super::{Outcome, Screen};
use crate::pixels::Pixels;
use crate::style::Style;
use crate::tree::{Node, Segment, Tree};
use crate::widgets::{Column, Picture};
use aethereal_actor::Answered;
use aethereal_render::Release;
use aethereal_render::placement::Resource;
use aethereal_render::texture::{TextureId, Textured};

/// A picture 4 by 2 of one gray.
fn gray() -> Pixels {
    let Ok(pixels) = Pixels::new(4, 2, vec![128; 32]) else {
        panic!("the pixels are four bytes a pixel");
    };

    pixels
}

/// A column of `count` pictures of `pixels`.
fn shown(pixels: &Pixels, count: u32) -> Tree {
    let pictures = (0..count)
        .map(|at| Node::new(Segment::Place(at), Picture { pixels: pixels.clone() }, Style::default(), Vec::new()))
        .collect();

    Tree::new(Node::new(Segment::Place(0), Column, Style::default(), pictures))
}

/// Lays `tree` out, answering each picture's texture made as the number 500 on, and every other draw made: each
/// layout's outcome, and every order sent.
fn pictured(screen: &mut Screen, tree: impl Fn() -> Tree) -> (Vec<Outcome>, Vec<Order>) {
    let mut outcomes = Vec::new();
    let mut every = Vec::new();
    let mut made = 500;
    for _ in 0..2 {
        let (outcome, orders) = laid(screen, tree(), &[]);
        for order in &orders {
            if let Order::Picture(context, _) = order {
                made += 1;
                let _ = screen.pictured(Answered::Yes(Textured::Made(id(made))), *context);
            }
        }
        outcomes.push(outcome);
        every.extend(orders);
    }

    (outcomes, every)
}

/// The textures the placements among `orders` bind.
fn bound(orders: &[Order]) -> Vec<TextureId> {
    orders
        .iter()
        .filter_map(|order| match order {
            Order::Place(place, _) => Some(place),
            _ => None,
        })
        .flat_map(|place| &place.joins)
        .flat_map(|join| &join.bindings)
        .filter_map(|bind| match bind.to {
            Resource::Texture(texture) => Some(texture),
            _ => None,
        })
        .collect()
}

#[test]
fn a_picture_shown_twice_is_made_once_and_drawn_once_its_texture_is_made() {
    // Catches a texture made for each widget showing a picture, and a layout drawn before its picture's texture is,
    // which binds a texture that does not exist.
    let mut screen = screen();
    let pixels = gray();

    let (outcomes, orders) = pictured(&mut screen, || shown(&pixels, 2));

    let asked = orders.iter().filter(|order| matches!(order, Order::Picture(..))).count();
    assert_eq!(asked, 1);
    assert_eq!(outcomes, vec![Outcome::Waiting, Outcome::Laid]);
    // Each widget draws its own placement, both from the one texture.
    let made: TextureId = id(501);
    assert_eq!(bound(&orders), vec![made, made]);
}

#[test]
fn a_picture_that_leaves_has_its_texture_released_after_the_list_that_named_it_is_replaced() {
    // Catches a texture kept after every widget showing it left, which leaks one for each image a transcript scrolls
    // past, and one released before the list drawing it is replaced, which the renderer would draw from.
    let mut screen = screen();
    let pixels = gray();
    let _ = pictured(&mut screen, || shown(&pixels, 1));

    let (_, orders) = laid(&mut screen, shown(&pixels, 0), &[]);

    let listed = orders.iter().position(|order| matches!(order, Order::List(_)));
    let released = orders.iter().position(|order| *order == Order::Release(Release::Texture(id(501))));
    let (Some(listed), Some(released)) = (listed, released) else {
        panic!("the list is sent and the texture released: {orders:?}");
    };
    assert!(listed < released, "{orders:?}");
}
