#![cfg(feature = "iterator")]

use serde::de::DeserializeOwned;

use cosmwasm_std::{from_slice, StdResult};
use cosmwasm_std::{Order, Storage, KV};

use crate::helpers::encode_length;

pub(crate) fn deserialize_kv<T: DeserializeOwned>(kv: KV) -> StdResult<KV<T>> {
    let (k, v) = kv;
    let t = from_slice::<T>(&v)?;
    Ok((k, t))
}

/// Calculates the raw key prefix for a given nested namespace
/// as documented in https://github.com/webmaster128/key-namespacing#nesting
pub(crate) fn to_length_prefixed_nested(namespaces: &[&[u8]]) -> Vec<u8> {
    let mut size = 0;
    for &namespace in namespaces {
        size += namespace.len() + 2;
    }

    let mut out = Vec::with_capacity(size);
    for &namespace in namespaces {
        out.extend_from_slice(&encode_length(namespace));
        out.extend_from_slice(namespace);
    }
    out
}

pub(crate) fn range_with_prefix<'a, S: Storage>(
    storage: &'a S,
    namespace: &[u8],
    start: Option<&[u8]>,
    end: Option<&[u8]>,
    order: Order,
) -> Box<dyn Iterator<Item = KV> + 'a> {
    // prepare start, end with prefix
    let start = match start {
        Some(s) => concat(namespace, s),
        None => namespace.to_vec(),
    };
    let end = match end {
        Some(e) => concat(namespace, e),
        // end is updating last byte by one
        None => namespace_upper_bound(namespace),
    };

    // get iterator from storage
    let base_iterator = storage.range(Some(&start), Some(&end), order);

    // make a copy for the closure to handle lifetimes safely
    let prefix = namespace.to_vec();
    let mapped = base_iterator.map(move |(k, v)| (trim(&prefix, &k), v));
    Box::new(mapped)
}

#[inline]
fn trim(namespace: &[u8], key: &[u8]) -> Vec<u8> {
    key[namespace.len()..].to_vec()
}

#[inline]
fn concat(namespace: &[u8], key: &[u8]) -> Vec<u8> {
    let mut k = namespace.to_vec();
    k.extend_from_slice(key);
    k
}

/// Returns a new vec of same length and last byte incremented by one
/// If last bytes are 255, we handle overflow up the chain.
/// If all bytes are 255, this returns wrong data - but that is never possible as a namespace
fn namespace_upper_bound(input: &[u8]) -> Vec<u8> {
    let mut copy = input.to_vec();
    // zero out all trailing 255, increment first that is not such
    for i in (0..input.len()).rev() {
        if copy[i] == 255 {
            copy[i] = 0;
        } else {
            copy[i] += 1;
            break;
        }
    }
    copy
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn to_length_prefixed_nested_works() {
        assert_eq!(to_length_prefixed_nested(&[]), b"");
        assert_eq!(to_length_prefixed_nested(&[b""]), b"\x00\x00");
        assert_eq!(to_length_prefixed_nested(&[b"", b""]), b"\x00\x00\x00\x00");

        assert_eq!(to_length_prefixed_nested(&[b"a"]), b"\x00\x01a");
        assert_eq!(
            to_length_prefixed_nested(&[b"a", b"ab"]),
            b"\x00\x01a\x00\x02ab"
        );
        assert_eq!(
            to_length_prefixed_nested(&[b"a", b"ab", b"abc"]),
            b"\x00\x01a\x00\x02ab\x00\x03abc"
        );
    }

    #[test]
    fn to_length_prefixed_nested_allows_many_long_namespaces() {
        // The 0xFFFF limit is for each namespace, not for the combination of them

        let long_namespace1 = vec![0xaa; 0xFFFD];
        let long_namespace2 = vec![0xbb; 0xFFFE];
        let long_namespace3 = vec![0xcc; 0xFFFF];

        let prefix =
            to_length_prefixed_nested(&[&long_namespace1, &long_namespace2, &long_namespace3]);
        assert_eq!(&prefix[0..2], b"\xFF\xFD");
        assert_eq!(&prefix[2..(2 + 0xFFFD)], long_namespace1.as_slice());
        assert_eq!(&prefix[(2 + 0xFFFD)..(2 + 0xFFFD + 2)], b"\xFF\xFe");
        assert_eq!(
            &prefix[(2 + 0xFFFD + 2)..(2 + 0xFFFD + 2 + 0xFFFE)],
            long_namespace2.as_slice()
        );
        assert_eq!(
            &prefix[(2 + 0xFFFD + 2 + 0xFFFE)..(2 + 0xFFFD + 2 + 0xFFFE + 2)],
            b"\xFF\xFf"
        );
        assert_eq!(
            &prefix[(2 + 0xFFFD + 2 + 0xFFFE + 2)..(2 + 0xFFFD + 2 + 0xFFFE + 2 + 0xFFFF)],
            long_namespace3.as_slice()
        );
    }

    #[test]
    fn to_length_prefixed_nested_calculates_capacity_correctly() {
        // Those tests cannot guarantee the required capacity was calculated correctly before
        // the vector allocation but increase the likelyhood of a proper implementation.

        let key = to_length_prefixed_nested(&[]);
        assert_eq!(key.capacity(), key.len());

        let key = to_length_prefixed_nested(&[b""]);
        assert_eq!(key.capacity(), key.len());

        let key = to_length_prefixed_nested(&[b"a"]);
        assert_eq!(key.capacity(), key.len());

        let key = to_length_prefixed_nested(&[b"a", b"bc"]);
        assert_eq!(key.capacity(), key.len());

        let key = to_length_prefixed_nested(&[b"a", b"bc", b"def"]);
        assert_eq!(key.capacity(), key.len());
    }
}

// currently disabled tests as they require a bunch of legacy non-sense
// TODO: enable
#[cfg(test)]
#[cfg(not(feature = "iterator"))]
mod namespace_test {
    use super::*;
    use cosmwasm_std::testing::MockStorage;

    #[test]
    fn test_range() {
        let mut storage = MockStorage::new();
        let prefix = to_length_prefixed(b"foo");
        let other_prefix = to_length_prefixed(b"food");

        // set some values in this range
        set_with_prefix(&mut storage, &prefix, b"bar", b"none");
        set_with_prefix(&mut storage, &prefix, b"snowy", b"day");

        // set some values outside this range
        set_with_prefix(&mut storage, &other_prefix, b"moon", b"buggy");

        // ensure we get proper result from prefixed_range iterator
        let mut iter = range_with_prefix(&storage, &prefix, None, None, Order::Descending);
        let first = iter.next().unwrap();
        assert_eq!(first, (b"snowy".to_vec(), b"day".to_vec()));
        let second = iter.next().unwrap();
        assert_eq!(second, (b"bar".to_vec(), b"none".to_vec()));
        assert!(iter.next().is_none());

        // ensure we get raw result from base range
        let iter = storage.range(None, None, Order::Ascending);
        assert_eq!(3, iter.count());

        // foo comes first
        let mut iter = storage.range(None, None, Order::Ascending);
        let first = iter.next().unwrap();
        let expected_key = concat(&prefix, b"bar");
        assert_eq!(first, (expected_key, b"none".to_vec()));
    }

    #[test]
    fn test_range_with_prefix_wrapover() {
        let mut storage = MockStorage::new();
        // if we don't properly wrap over there will be issues here (note 255+1 is used to calculate end)
        let prefix = to_length_prefixed(b"f\xff\xff");
        let other_prefix = to_length_prefixed(b"f\xff\x44");

        // set some values in this range
        set_with_prefix(&mut storage, &prefix, b"bar", b"none");
        set_with_prefix(&mut storage, &prefix, b"snowy", b"day");

        // set some values outside this range
        set_with_prefix(&mut storage, &other_prefix, b"moon", b"buggy");

        // ensure we get proper result from prefixed_range iterator
        let iter = range_with_prefix(&storage, &prefix, None, None, Order::Descending);
        let elements: Vec<KV> = iter.collect();
        assert_eq!(
            elements,
            vec![
                (b"snowy".to_vec(), b"day".to_vec()),
                (b"bar".to_vec(), b"none".to_vec()),
            ]
        );
    }

    #[test]
    fn test_range_with_start_end_set() {
        let mut storage = MockStorage::new();
        // if we don't properly wrap over there will be issues here (note 255+1 is used to calculate end)
        let prefix = to_length_prefixed(b"f\xff\xff");
        let other_prefix = to_length_prefixed(b"f\xff\x44");

        // set some values in this range
        set_with_prefix(&mut storage, &prefix, b"bar", b"none");
        set_with_prefix(&mut storage, &prefix, b"snowy", b"day");

        // set some values outside this range
        set_with_prefix(&mut storage, &other_prefix, b"moon", b"buggy");

        // make sure start and end are applied properly
        let res: Vec<KV> =
            range_with_prefix(&storage, &prefix, Some(b"b"), Some(b"c"), Order::Ascending)
                .collect();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0], (b"bar".to_vec(), b"none".to_vec()));

        // make sure start and end are applied properly
        let res: Vec<KV> = range_with_prefix(
            &storage,
            &prefix,
            Some(b"bas"),
            Some(b"sno"),
            Order::Ascending,
        )
        .collect();
        assert_eq!(res.len(), 0);

        let res: Vec<KV> =
            range_with_prefix(&storage, &prefix, Some(b"ant"), None, Order::Ascending).collect();
        assert_eq!(res.len(), 2);
        assert_eq!(res[0], (b"bar".to_vec(), b"none".to_vec()));
        assert_eq!(res[1], (b"snowy".to_vec(), b"day".to_vec()));
    }

    #[test]
    fn test_namespace_upper_bound() {
        assert_eq!(namespace_upper_bound(b"bob"), b"boc".to_vec());
        assert_eq!(namespace_upper_bound(b"fo\xfe"), b"fo\xff".to_vec());
        assert_eq!(namespace_upper_bound(b"fo\xff"), b"fp\x00".to_vec());
        // multiple \xff roll over
        assert_eq!(
            namespace_upper_bound(b"fo\xff\xff\xff"),
            b"fp\x00\x00\x00".to_vec()
        );
        // \xff not at the end are ignored
        assert_eq!(namespace_upper_bound(b"\xffabc"), b"\xffabd".to_vec());
    }
}