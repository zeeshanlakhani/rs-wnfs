use criterion::{
    async_executor::AsyncStdExecutor, black_box, criterion_group, criterion_main, BatchSize,
    Criterion, Throughput,
};
use proptest::{arbitrary::any, collection::vec, test_runner::TestRunner};
use std::{rc::Rc, sync::Arc};
use wnfs::{
    dagcbor,
    private::{
        hamt::{Hamt, Node},
        strategies::{node_from_operations, operations},
    },
    utils::Sampleable,
    BlockStore, MemoryBlockStore,
};

fn node_set(c: &mut Criterion) {
    let mut runner = TestRunner::deterministic();
    let mut store = MemoryBlockStore::default();
    let operations = operations(any::<[u8; 32]>(), any::<u64>(), 1_000_000).sample(&mut runner);
    let node =
        &async_std::task::block_on(async { node_from_operations(operations, &mut store).await })
            .expect("Couldn't setup HAMT node from operations");

    let store = Arc::new(store);

    c.bench_function("node set", |b| {
        b.to_async(AsyncStdExecutor).iter_batched(
            || {
                let store = Arc::clone(&store);
                let kv = (any::<[u8; 32]>(), any::<u64>()).sample(&mut runner);
                (store, kv)
            },
            |(store, (key, value))| async move {
                black_box(
                    Rc::clone(node)
                        .set(key, value, store.as_ref())
                        .await
                        .unwrap(),
                );
            },
            BatchSize::SmallInput,
        );
    });
}

fn node_set_consecutive(c: &mut Criterion) {
    let mut runner = TestRunner::deterministic();

    c.bench_function("node set 1000 consecutive", |b| {
        b.to_async(AsyncStdExecutor).iter_batched(
            || {
                let mut store = MemoryBlockStore::default();
                let operations =
                    operations(any::<[u8; 32]>(), any::<u64>(), 1000).sample(&mut runner);
                let node = async_std::task::block_on(async {
                    node_from_operations(operations, &mut store).await
                })
                .expect("Couldn't setup HAMT node from operations");

                let kvs = vec((any::<[u8; 32]>(), any::<u64>()), 1000).sample(&mut runner);
                (node, store, kvs)
            },
            |(mut node, store, kvs)| async move {
                for (key, value) in kvs {
                    node = black_box(node.set(key, value, &store).await.unwrap());
                }
            },
            BatchSize::SmallInput,
        );
    });
}

fn node_load_get(c: &mut Criterion) {
    let mut store = MemoryBlockStore::default();
    let cid = async_std::task::block_on(async {
        let mut node = Rc::new(<Node<_, _>>::default());
        for i in 0..50 {
            node = node.set(i.to_string(), i, &mut store).await.unwrap();
        }

        let encoded_hamt = dagcbor::async_encode(&Hamt::with_root(node), &mut store)
            .await
            .unwrap();

        let cid = store.put_serializable(&encoded_hamt).await.unwrap();

        cid
    });

    c.bench_function("node load and get", |b| {
        b.to_async(AsyncStdExecutor).iter(|| async {
            let encoded_hamt = store.get_deserializable::<Vec<u8>>(&cid).await.unwrap();
            let hamt: Hamt<String, i32> = dagcbor::decode(encoded_hamt.as_ref()).unwrap();

            for i in 0..50 {
                assert!(hamt
                    .root
                    .get(&i.to_string(), &store)
                    .await
                    .unwrap()
                    .is_some());
            }
        })
    });
}

fn node_load_remove(c: &mut Criterion) {
    let mut store = MemoryBlockStore::default();
    let cid = async_std::task::block_on(async {
        let mut node = Rc::new(<Node<_, _>>::default());
        for i in 0..50 {
            node = node.set(i.to_string(), i, &mut store).await.unwrap();
        }

        let encoded_hamt = dagcbor::async_encode(&Hamt::with_root(node), &mut store)
            .await
            .unwrap();

        let cid = store.put_serializable(&encoded_hamt).await.unwrap();

        cid
    });

    c.bench_function("node load and remove", |b| {
        b.to_async(AsyncStdExecutor).iter(|| async {
            let encoded_hamt = store.get_deserializable::<Vec<u8>>(&cid).await.unwrap();
            let mut hamt: Hamt<String, i32> =
                black_box(dagcbor::decode(encoded_hamt.as_ref()).unwrap());

            for i in 0..50 {
                let (root, value) = hamt.root.remove(&i.to_string(), &store).await.unwrap();
                assert!(value.is_some());
                hamt.root = root;
            }
        })
    });
}

fn hamt_load_decode(c: &mut Criterion) {
    let mut store = MemoryBlockStore::default();
    let (cid, bytes) = async_std::task::block_on(async {
        let mut node = Rc::new(<Node<_, _>>::default());
        for i in 0..50 {
            node = node.set(i.to_string(), i, &mut store).await.unwrap();
        }

        let encoded_hamt = dagcbor::async_encode(&Hamt::with_root(node), &mut store)
            .await
            .unwrap();

        let cid = store.put_serializable(&encoded_hamt).await.unwrap();

        (cid, encoded_hamt)
    });

    let mut group = c.benchmark_group("hamt load and decode");
    group.throughput(Throughput::Bytes(bytes.len() as u64));
    group.bench_function("0", |b| {
        b.to_async(AsyncStdExecutor).iter(|| async {
            let encoded_hamt = store.get_deserializable::<Vec<u8>>(&cid).await.unwrap();
            let _: Hamt<String, i32> = black_box(dagcbor::decode(encoded_hamt.as_ref()).unwrap());
        })
    });
    group.finish();
}

fn hamt_set_encode(c: &mut Criterion) {
    c.bench_function("hamt set and encode", |b| {
        b.to_async(AsyncStdExecutor).iter_batched(
            || {
                (
                    MemoryBlockStore::default(),
                    Rc::new(<Node<_, _>>::default()),
                )
            },
            |(mut store, mut node)| async move {
                for i in 0..50 {
                    node = node.set(i.to_string(), i, &mut store).await.unwrap();
                }

                let hamt = Hamt::with_root(node);

                let _ = black_box(dagcbor::async_encode(&hamt, &mut store).await.unwrap());
            },
            BatchSize::SmallInput,
        )
    });
}

criterion_group!(
    benches,
    node_set,
    node_set_consecutive,
    node_load_get,
    node_load_remove,
    hamt_load_decode,
    hamt_set_encode
);

criterion_main!(benches);
