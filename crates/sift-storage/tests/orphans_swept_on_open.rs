use sift_storage::Store;

#[tokio::test]
async fn compacting_parquet_is_swept_on_open() {
    let tmp = tempfile::tempdir().unwrap();
    let segs = tmp.path().join("segments");
    tokio::fs::create_dir_all(&segs).await.unwrap();
    let orphan = segs.join("2026-04-23T12-0000000001.compacting.parquet");
    tokio::fs::write(&orphan, b"garbage").await.unwrap();

    let _store = Store::open(tmp.path()).await.unwrap();
    assert!(!orphan.exists(), "orphan .compacting.parquet should be deleted");
}

#[tokio::test]
async fn non_current_wal_is_swept_on_open() {
    let tmp = tempfile::tempdir().unwrap();
    let wal = tmp.path().join("wal");
    tokio::fs::create_dir_all(&wal).await.unwrap();
    tokio::fs::write(wal.join("current"), b"0000000001.log")
        .await
        .unwrap();
    tokio::fs::write(wal.join("0000000001.log"), b"")
        .await
        .unwrap();
    let orphan = wal.join("0000000099.log");
    tokio::fs::write(&orphan, b"garbage").await.unwrap();

    let _store = Store::open(tmp.path()).await.unwrap();
    assert!(!orphan.exists(), "orphan WAL file should be deleted");
}
