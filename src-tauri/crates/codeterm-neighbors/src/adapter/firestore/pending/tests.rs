use super::*;

#[test]
fn pending_count_is_bounded_and_scoped_to_one_friendship_without_a_delivery_cursor() {
    let friendship = "a".repeat(64);
    let query = count_query(&friendship);
    assert!(matches!(
        query.aggregations[0].operator.as_ref(),
        Some(
            fs::structured_aggregation_query::aggregation::Operator::Count(
                fs::structured_aggregation_query::aggregation::Count { up_to: Some(100) }
            )
        )
    ));
    let Some(fs::structured_aggregation_query::QueryType::StructuredQuery(query)) =
        query.query_type
    else {
        panic!("query");
    };
    assert_eq!(query.r#where, Some(friendship_filter(&friendship)));
    assert!(query.start_at.is_none());
    assert!(query.limit.is_none());
    let result = |count| fs::AggregationResult {
        aggregate_fields: [(
            "pending".into(),
            value(fs::value::ValueType::IntegerValue(count)),
        )]
        .into_iter()
        .collect(),
    };
    assert_eq!(parse_count(result(0)), Ok(0));
    assert_eq!(parse_count(result(12000)), Ok(100));
    assert_eq!(parse_count(result(-1)), Err(Error::Transport));
    assert_eq!(
        parse_count(fs::AggregationResult::default()),
        Err(Error::Transport)
    );
}
