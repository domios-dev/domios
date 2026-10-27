use super::*;

pub(in crate::adapter) async fn count(
    transport: &FirebaseTransport,
    friendship: &str,
) -> Result<u64> {
    if !hex_id(friendship, 64) {
        return Err(Error::InvalidEnvelope);
    }
    let (mut client, token, database) = connection(transport).await?;
    let parent = format!("{database}/documents/friendInbox/{}", transport.uid);
    let query = count_query(friendship);
    let mut request = authenticated_request(
        &token,
        &database,
        fs::RunAggregationQueryRequest {
            parent: parent.clone(),
            query_type: Some(
                fs::run_aggregation_query_request::QueryType::StructuredAggregationQuery(query),
            ),
            ..Default::default()
        },
    )?;
    request.set_timeout(Duration::from_secs(15));
    let mut stream = client
        .run_aggregation_query(request)
        .await
        .map_err(|status| rpc_failure(transport.calls.as_ref(), "inbox.count", &parent, status))?
        .into_inner();
    let mut count = None;
    while let Some(response) = stream.message().await.map_err(|status| {
        rpc_failure(
            transport.calls.as_ref(),
            "inbox.count.read",
            &parent,
            status,
        )
    })? {
        if let Some(result) = response.result {
            count = Some(parse_count(result)?);
        } else if count.is_none() && response.read_time.is_some() {
            count = Some(0);
        }
    }
    count.ok_or(Error::Transport)
}

fn count_query(friendship: &str) -> fs::StructuredAggregationQuery {
    use fs::structured_aggregation_query::{
        self as aggregation,
        aggregation::{Count, Operator},
    };
    fs::StructuredAggregationQuery {
        query_type: Some(aggregation::QueryType::StructuredQuery(
            fs::StructuredQuery {
                from: vec![fs::structured_query::CollectionSelector {
                    collection_id: "messages".into(),
                    all_descendants: false,
                }],
                r#where: Some(friendship_filter(friendship)),
                ..Default::default()
            },
        )),
        aggregations: vec![aggregation::Aggregation {
            alias: "pending".into(),
            operator: Some(Operator::Count(Count {
                up_to: Some(super::super::PENDING_COUNT_LIMIT as i64),
            })),
        }],
    }
}

fn parse_count(result: fs::AggregationResult) -> Result<u64> {
    match result
        .aggregate_fields
        .get("pending")
        .and_then(|value| value.value_type.as_ref())
    {
        Some(fs::value::ValueType::IntegerValue(count)) if *count >= 0 => {
            Ok((*count as u64).min(super::super::PENDING_COUNT_LIMIT))
        }
        _ => Err(Error::Transport),
    }
}

#[cfg(test)]
mod tests;
