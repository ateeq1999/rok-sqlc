//! Column types: intervals, bytes, and times with time zone, enums, and
//! value lists.

use super::*;

/// `INTERVAL`, `BYTEA`, and `TIME WITH TIME ZONE` columns (and their arrays)
/// round-trip through create, get, update, and search, and query-file casts
/// to them bind and decode the mapped types.
pub(super) async fn interval_bytea_and_timetz_round_trip(pool: &PgPool) {
    use chrono::{FixedOffset, NaiveTime};
    use rok_sqlc_fixture_app::plain_db;
    use plain_db::search::{Equality, Page, Sort};
    use plain_db::timed_badges::filter::{TimedBadgeFilter, TimedBadgeSortKey};
    use plain_db::timed_badges::models::{NewTimedBadge, UpdateTimedBadge};
    use plain_db::timed_badges::{read, write};
    use sqlx::postgres::types::{PgInterval, PgTimeTz};

    let interval = |days, microseconds| PgInterval {
        months: 1,
        days,
        microseconds,
    };
    let time_tz = |hour, offset_hours| PgTimeTz {
        time: NaiveTime::from_hms_opt(hour, 30, 0).unwrap(),
        offset: FixedOffset::east_opt(offset_hours * 3600).unwrap(),
    };

    let created = write::create(
        pool,
        NewTimedBadge {
            span: interval(2, 1_500_000),
            breaks: Some(vec![interval(0, 60_000_000), interval(1, 0)]),
            badge: vec![0, 159, 146, 150, 255],
            photos: Some(vec![vec![1, 2], Vec::new()]),
            opens: time_tz(9, 2),
            closes: None,
            slots: Some(vec![time_tz(10, -5)]),
        },
    )
    .await
    .unwrap();
    assert_eq!(created.span, interval(2, 1_500_000));
    assert_eq!(created.badge, [0, 159, 146, 150, 255]);
    assert_eq!(created.photos, Some(vec![vec![1, 2], Vec::new()]));
    assert_eq!(created.opens, time_tz(9, 2));
    assert_eq!(created.slots, Some(vec![time_tz(10, -5)]));
    let other = write::create(
        pool,
        NewTimedBadge {
            span: interval(5, 0),
            breaks: None,
            badge: vec![7],
            photos: None,
            opens: time_tz(8, 0),
            closes: Some(time_tz(17, 0)),
            slots: None,
        },
    )
    .await
    .unwrap();

    assert_eq!(
        read::get_by_id(pool, created.id).await.unwrap().as_ref(),
        Some(&created)
    );
    // A `BYTEA` lookup borrows its key as a byte slice.
    let by_badge = read::get_by_badge(pool, &[7]).await.unwrap();
    assert_eq!(by_badge.map(|row| row.id), Some(other.id));

    let updated = write::update(
        pool,
        created.id,
        UpdateTimedBadge {
            span: Some(interval(3, 0)),
            badge: Some(vec![42; 3]),
            closes: Some(Some(time_tz(18, 2))),
            photos: Some(None),
            ..UpdateTimedBadge::default()
        },
    )
    .await
    .unwrap()
    .expect("the row exists");
    assert_eq!(updated.span, interval(3, 0));
    assert_eq!(updated.badge, [42, 42, 42]);
    assert_eq!(updated.closes, Some(time_tz(18, 2)));
    assert_eq!(updated.photos, None);
    assert_eq!(updated.breaks, created.breaks);

    let found = |filter| async move {
        read::search(pool, filter, Sort::asc(TimedBadgeSortKey::Span), Page::first(10))
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        found(TimedBadgeFilter {
            span: vec![Equality::Eq(interval(5, 0))],
            ..TimedBadgeFilter::default()
        })
        .await,
        [other.id]
    );
    assert_eq!(
        found(TimedBadgeFilter {
            badge: vec![Equality::In(vec![vec![42; 3], vec![9]])],
            ..TimedBadgeFilter::default()
        })
        .await,
        [created.id]
    );
    assert_eq!(
        found(TimedBadgeFilter {
            opens: vec![Equality::Ne(time_tz(9, 2))],
            closes_is_null: Some(false),
            ..TimedBadgeFilter::default()
        })
        .await,
        [other.id]
    );
    // Sorting by an interval column orders by PostgreSQL's interval order.
    assert_eq!(
        found(TimedBadgeFilter::default()).await,
        [created.id, other.id]
    );

    let rows = plain_db::queries::badges_at_least(pool, interval(0, 0), interval(3, 0), &[7])
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, created.id);
    assert_eq!(rows[0].badge, [42, 42, 42]);
    // `$1` adds its month too.
    assert_eq!(
        rows[0].extended,
        Some(PgInterval {
            months: 2,
            days: 3,
            microseconds: 0,
        })
    );
    let opening = plain_db::queries::badge_opening(pool, time_tz(8, 0))
        .await
        .unwrap();
    assert_eq!(opening, Some(time_tz(8, 0)));
}

/// `ticket_priority` and `app.ticket_state` (extended by alter_schema.sql)
/// are database enums; their columns use the generated `db::types` enums in
/// rows, inputs, patches, filters, sort keys, arrays, and query files.
pub(super) async fn enums_keep_their_exact_labels(pool: &PgPool) {
    use db::queries::tickets::{tickets_in_states, tickets_with_priority};
    use db::search::{Equality, Page, Sort};
    use db::tickets::filter::{TicketFilter, TicketSortKey};
    use db::tickets::models::{NewTicket, Ticket, UpdateTicket};
    use db::tickets::{read, write};
    use db::types::{TicketPriority, TicketState};

    let urgent = write::create(
        pool,
        NewTicket {
            priority: TicketPriority::VeryHigh,
            state: Some(TicketState::Closed),
            watch_levels: Some(vec![TicketPriority::Low, TicketPriority::Medium]),
        },
    )
    .await
    .unwrap();
    let routine = write::create(
        pool,
        NewTicket {
            priority: TicketPriority::High,
            state: None,
            watch_levels: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(routine.watch_levels, [], "the array default applies");

    let stored = read::get_by_id(pool, urgent.id).await.unwrap().unwrap();
    assert_eq!(
        stored,
        Ticket {
            id: urgent.id,
            priority: TicketPriority::VeryHigh,
            state: Some(TicketState::Closed),
            watch_levels: vec![TicketPriority::Low, TicketPriority::Medium],
        }
    );
    let labels: (String, Option<String>, Vec<String>) = sqlx::query_as(
        "SELECT priority::text, state::text, watch_levels::text[] FROM app.tickets WHERE id = $1",
    )
    .bind(urgent.id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        labels,
        (
            "Very High".to_owned(),
            Some("CLOSED".to_owned()),
            vec!["Low".to_owned(), "MEDIUM".to_owned()],
        ),
        "the database stores each variant's exact label"
    );

    // `Archived` exists only because of ALTER TYPE ... ADD VALUE.
    let archived = write::update(
        pool,
        urgent.id,
        UpdateTicket {
            state: Some(Some(TicketState::Archived)),
            watch_levels: Some(vec![TicketPriority::High]),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(archived.state, Some(TicketState::Archived));
    assert_eq!(archived.watch_levels, [TicketPriority::High]);

    let very_high = read::search(
        pool,
        TicketFilter {
            priority: vec![Equality::Eq(TicketPriority::VeryHigh)],
            ..Default::default()
        },
        Sort::asc(TicketSortKey::Id),
        Page::first(10),
    )
    .await
    .unwrap();
    assert_eq!(very_high, std::slice::from_ref(&archived));

    let not_archived = read::search(
        pool,
        TicketFilter {
            state: vec![Equality::NotIn(vec![TicketState::Archived])],
            ..Default::default()
        },
        Sort::asc(TicketSortKey::Id),
        Page::first(10),
    )
    .await
    .unwrap();
    assert!(not_archived.is_empty(), "a NULL state matches no condition");

    // Enums sort in declaration order: `high` comes before `Very High`.
    let by_priority = read::search(
        pool,
        TicketFilter::default(),
        Sort::desc(TicketSortKey::Priority),
        Page::first(10),
    )
    .await
    .unwrap();
    let priorities: Vec<TicketPriority> =
        by_priority.iter().map(|ticket| ticket.priority).collect();
    assert_eq!(priorities, [TicketPriority::VeryHigh, TicketPriority::High]);

    let found = tickets_with_priority(pool, TicketPriority::High)
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, routine.id);
    assert_eq!(found[0].priority, TicketPriority::High);
    assert_eq!(found[0].state, None);

    // Casts to enum types (`$1::app.ticket_state[]`, `$2::ticket_priority`)
    // type the parameters and the cast result column.
    let in_states = tickets_in_states(
        pool,
        &[TicketState::Archived, TicketState::Open],
        TicketPriority::Low,
    )
    .await
    .unwrap();
    assert_eq!(in_states.len(), 1);
    assert_eq!(in_states[0].id, urgent.id);
    assert_eq!(in_states[0].state, Some(TicketState::Archived));
    assert_eq!(in_states[0].wanted, Some(TicketPriority::Low));
}

/// `ticket_channel` is renamed to `contact_channel` by alter_schema.sql: the
/// enum is `ContactChannel`, bound and decoded under its new database name.
pub(super) async fn renamed_enum_types_use_their_new_name(pool: &PgPool) {
    use db::queries::tickets::contacts_on_channel;
    use db::ticket_contacts::models::{NewTicketContact, TicketContact};
    use db::ticket_contacts::{read, write};
    use db::types::ContactChannel;

    let phone = write::create(
        pool,
        NewTicketContact {
            channel: ContactChannel::Phone,
            fallbacks: Some(vec![ContactChannel::Email]),
        },
    )
    .await
    .unwrap();
    let email = write::create(
        pool,
        NewTicketContact {
            channel: ContactChannel::Email,
            fallbacks: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        read::get_by_id(pool, phone.id).await.unwrap(),
        Some(TicketContact {
            id: phone.id,
            channel: ContactChannel::Phone,
            fallbacks: Some(vec![ContactChannel::Email]),
        })
    );
    let label: String =
        sqlx::query_scalar("SELECT channel::text FROM ticket_contacts WHERE id = $1")
            .bind(phone.id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(label, "Phone");

    let reachable = contacts_on_channel(pool, ContactChannel::Email)
        .await
        .unwrap();
    assert_eq!(reachable, [phone.id, email.id]);
    let by_phone = contacts_on_channel(pool, ContactChannel::Phone)
        .await
        .unwrap();
    assert_eq!(by_phone, [phone.id]);
}

/// `app.shipments.status`, `app.shipments.carrier`, `refunds.status`, and
/// `refunds.reason` (added by alter_schema.sql) are value-list columns: text
/// columns restricted by `CHECK (col IN (...))`. Each becomes its own enum in
/// `db::types`, encoded as text with each exact value.
pub(super) async fn value_list_columns_round_trip(pool: &PgPool) {
    use db::queries::shipments::shipments_with_status;
    use db::refunds::models::NewRefund;
    use db::search::{Equality, Page, Sort};
    use db::shipments::filter::{ShipmentFilter, ShipmentSortKey};
    use db::shipments::models::{NewShipment, Shipment, UpdateShipment};
    use db::shipments::{read, write};
    use db::types::{RefundReason, RefundStatus, ShipmentCarrier, ShipmentStatus};

    let defaulted = write::create(
        pool,
        NewShipment {
            status: None,
            carrier: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        defaulted.status,
        ShipmentStatus::Pending,
        "the default applies"
    );
    assert_eq!(defaulted.carrier, None);

    let moving = write::create(
        pool,
        NewShipment {
            status: Some(ShipmentStatus::InTransit),
            carrier: Some(ShipmentCarrier::Dhl),
        },
    )
    .await
    .unwrap();
    let stored = read::get_by_id(pool, moving.id).await.unwrap().unwrap();
    assert_eq!(
        stored,
        Shipment {
            id: moving.id,
            status: ShipmentStatus::InTransit,
            carrier: Some(ShipmentCarrier::Dhl),
        }
    );
    let text: (String, Option<String>) =
        sqlx::query_as("SELECT status, carrier FROM app.shipments WHERE id = $1")
            .bind(moving.id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(
        text,
        ("In Transit".to_owned(), Some("dhl".to_owned())),
        "the database stores each variant's exact value"
    );

    let delivered = write::update(
        pool,
        moving.id,
        UpdateShipment {
            status: Some(ShipmentStatus::Delivered),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(delivered.status, ShipmentStatus::Delivered);
    assert_eq!(ShipmentStatus::Delivered.as_str(), "DELIVERED");

    let found = read::search(
        pool,
        ShipmentFilter {
            status: vec![Equality::In(vec![
                ShipmentStatus::Delivered,
                ShipmentStatus::InTransit,
            ])],
            carrier: vec![Equality::Eq(ShipmentCarrier::Dhl)],
            ..Default::default()
        },
        Sort::asc(ShipmentSortKey::Status),
        Page::first(10),
    )
    .await
    .unwrap();
    assert_eq!(found, std::slice::from_ref(&delivered));

    let pending = shipments_with_status(pool, ShipmentStatus::Pending)
        .await
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, defaulted.id);
    assert_eq!(pending[0].status, ShipmentStatus::Pending);

    // Only the listed values exist in Rust: text outside the list neither
    // parses nor decodes.
    assert!("delivered".parse::<ShipmentStatus>().is_err());
    assert_eq!(
        "In Transit".parse::<ShipmentStatus>(),
        Ok(ShipmentStatus::InTransit)
    );
    let decoded = sqlx::query_scalar::<_, ShipmentStatus>("SELECT 'lost'::text")
        .fetch_one(pool)
        .await;
    assert!(decoded.is_err(), "an unknown value does not decode");

    // Same column name, different table: a distinct enum.
    let refund = db::refunds::write::create(
        pool,
        NewRefund {
            status: RefundStatus::Requested,
            reason: Some(RefundReason::Late),
        },
    )
    .await
    .unwrap();
    assert_eq!(refund.status, RefundStatus::Requested);
    assert_eq!(refund.reason, Some(RefundReason::Late));
}
