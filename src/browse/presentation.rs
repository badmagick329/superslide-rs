//! How the image list's order is worded in the notice each sort key shows.

use crate::browse::domain::{Order, SortKey, SortMode};

/// Names the whole order, not just the key pressed, so the notice also says what stayed on.
pub fn describe(order: Order) -> String {
    let mut text = match order.mode {
        SortMode::By(SortKey::Name) => "Sorted by name",
        SortMode::By(SortKey::Date) => "Sorted by date",
        SortMode::By(SortKey::Size) => "Sorted by size",
        SortMode::Shuffle { .. } => "Shuffled",
    }
    .to_owned();
    if order.reversed {
        text += ", reversed";
    }
    if order.grouped {
        text += ", grouped by folder";
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notice_names_every_part_of_the_order() {
        let date = Order::new(SortMode::By(SortKey::Date));
        assert_eq!(describe(date), "Sorted by date");
        let all = Order {
            reversed: true,
            grouped: true,
            ..date
        };
        assert_eq!(describe(all), "Sorted by date, reversed, grouped by folder");
        assert_eq!(
            describe(Order::new(SortMode::Shuffle { seed: 1 })),
            "Shuffled"
        );
    }
}
