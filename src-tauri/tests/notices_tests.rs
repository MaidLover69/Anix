//! The notification centre's store: pushing, listing by kind, unread count and marking read.

use anix_lib::db;
use anix_lib::notices;

#[tokio::test]
async fn notices_push_list_and_mark_read() {
    let pool = db::init_db_in_memory().await.expect("in-memory database");
    assert_eq!(notices::unread_count(&pool).await.unwrap(), 0);

    let test = notices::push(&pool, notices::TEST, "Test notification", "Body", Some("/notifications")).await.unwrap();
    assert_eq!(notices::unread_count(&pool).await.unwrap(), 1);
    let airing = notices::push(&pool, notices::AIRING, "Episode 3 is out", "", Some("/detail/1")).await.unwrap();
    notices::push(&pool, notices::DOWNLOAD, "Download failed", "Episode 2", None).await.unwrap();
    assert_eq!(notices::unread_count(&pool).await.unwrap(), 3);

    let all = notices::list(&pool, None).await.unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].title, "Download failed", "newest first");
    assert!(all.iter().all(|n| !n.read));
    let only_airing = notices::list(&pool, Some(notices::AIRING)).await.unwrap();
    assert_eq!(only_airing.len(), 1);
    assert_eq!((only_airing[0].id, only_airing[0].link.as_deref()), (airing, Some("/detail/1")));

    assert_eq!(notices::mark_read(&pool, Some(&[test])).await.unwrap(), 1);
    assert_eq!(notices::mark_read(&pool, Some(&[test])).await.unwrap(), 0);
    assert_eq!(notices::unread_count(&pool).await.unwrap(), 2);
    assert_eq!(notices::mark_read(&pool, None).await.unwrap(), 2);
    assert_eq!(notices::unread_count(&pool).await.unwrap(), 0);
    assert!(notices::list(&pool, None).await.unwrap().iter().all(|n| n.read));

    notices::clear(&pool).await.unwrap();
    assert!(notices::list(&pool, None).await.unwrap().is_empty());
}

#[tokio::test]
async fn notices_keep_only_the_newest() {
    let pool = db::init_db_in_memory().await.expect("in-memory database");
    for n in 0..(notices::KEEP + 5) {
        notices::push(&pool, "general", &format!("Notice {}", n), "", None).await.unwrap();
    }
    let all = notices::list(&pool, None).await.unwrap();
    assert_eq!(all.len() as i64, notices::KEEP);
    assert_eq!(all[0].title, format!("Notice {}", notices::KEEP + 4));
}
