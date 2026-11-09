//! Collection bounds for selected protocol fields, not a global memory budget.
use serde::de::{self, DeserializeSeed, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::{fmt, marker::PhantomData};

struct RejectExtra;
impl<'de> DeserializeSeed<'de> for RejectExtra {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, _deserializer: D) -> Result<(), D::Error> {
        // Do not deserialize or skip the surplus value, even if it is malformed.
        Err(de::Error::custom("IPC collection limit exceeded"))
    }
}

struct Bounded<T, F, const LIMIT: usize> {
    check: F,
    marker: PhantomData<T>,
}
impl<'de, T, F, const LIMIT: usize> Visitor<'de> for Bounded<T, F, LIMIT>
where
    T: Deserialize<'de>,
    F: FnMut(&T) -> Result<(), &'static str>,
{
    type Value = Vec<T>;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "a sequence of at most {LIMIT} entries")
    }
    fn visit_seq<A: SeqAccess<'de>>(mut self, mut sequence: A) -> Result<Vec<T>, A::Error> {
        // Never trust size_hint. Requested capacity grows geometrically but is
        // capped at LIMIT; allocator rounding is not an exact RAM guarantee.
        let mut values = Vec::new();
        while values.len() < LIMIT {
            let Some(value) = sequence.next_element::<T>()? else {
                return Ok(values);
            };
            (self.check)(&value).map_err(de::Error::custom)?;
            if values.len() == values.capacity() {
                let target = values.len().saturating_mul(2).max(1).min(LIMIT);
                values
                    .try_reserve_exact(target - values.len())
                    .map_err(de::Error::custom)?;
            }
            values.push(value);
        }
        sequence.next_element_seed(RejectExtra)?;
        Ok(values)
    }
}

pub(super) fn deserialize<'de, D, T, const LIMIT: usize>(
    deserializer: D,
) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    deserializer.deserialize_seq(Bounded::<T, _, LIMIT> {
        check: |_: &T| Ok(()),
        marker: PhantomData,
    })
}

pub(super) fn display_items<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<crate::DisplayItem>, D::Error> {
    let mut controls = 0usize;
    deserializer.deserialize_seq(
        Bounded::<crate::DisplayItem, _, { crate::MAX_DISPLAY_ITEMS }> {
            check: |item: &crate::DisplayItem| {
                if matches!(item, crate::DisplayItem::Control { .. }) {
                    controls += 1;
                    if controls > crate::MAX_PAGE_CONTROLS {
                        return Err("IPC control count exceeded");
                    }
                }
                Ok(())
            },
            marker: PhantomData,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug, Deserialize)]
    struct Small(#[serde(deserialize_with = "deserialize::<_, _, 2>")] Vec<u8>);

    #[test]
    fn exact_limit_and_surplus_including_malformed_value() {
        assert_eq!(
            serde_json::from_str::<Small>("[1,2]").unwrap().0,
            vec![1, 2]
        );
        assert!(
            serde_json::from_str::<Small>("[1,2,3]")
                .unwrap_err()
                .to_string()
                .contains("collection limit")
        );
        assert!(
            serde_json::from_str::<Small>("[1,2,{\"unfinished\":")
                .unwrap_err()
                .to_string()
                .contains("collection limit")
        );
    }

    #[test]
    fn surplus_element_is_never_deserialized() {
        static CALLS: AtomicUsize = AtomicUsize::new(0);
        struct Counted;
        impl<'de> Deserialize<'de> for Counted {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                CALLS.fetch_add(1, Ordering::SeqCst);
                u8::deserialize(d).map(|_| Self)
            }
        }
        let mut d = serde_json::Deserializer::from_str("[1,2,3]");
        assert!(deserialize::<_, Counted, 2>(&mut d).is_err());
        assert_eq!(CALLS.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn hostile_size_hint_does_not_drive_allocation() {
        struct Empty;
        impl<'de> SeqAccess<'de> for Empty {
            type Error = serde::de::value::Error;
            fn next_element_seed<T: DeserializeSeed<'de>>(
                &mut self,
                _: T,
            ) -> Result<Option<T::Value>, Self::Error> {
                Ok(None)
            }
            fn size_hint(&self) -> Option<usize> {
                Some(usize::MAX)
            }
        }
        let values = Bounded::<u8, _, 2> {
            check: |_: &u8| Ok(()),
            marker: PhantomData,
        }
        .visit_seq(Empty)
        .unwrap();
        assert_eq!(values.capacity(), 0);
    }
}
