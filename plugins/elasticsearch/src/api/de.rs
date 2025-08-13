use serde::Deserialize;

/// A response to a "create" query with the following filter (set as a query parameter):
/// `filter_path=items.*.error`
///
/// # Example response
///
/// ```json
/// {
/// "items": [
///     {
///       "create": {
///         "error": {
///           "type": "version_conflict_engine_exception",
///           "reason": "[tt1392214]: version conflict, document already exists",
///           "index": "movies",
///           "index_uuid": "yhizhusbSWmP0G7OJnmcLg",
///           "shard": "0",
///         }
///       }
///     }
/// }
/// ```
///
/// # Unfiltered example response
///
/// This is not what we are parsing here, but it shows the difference.
///
/// ```json
/// {
///     "took": 30,
///     "errors": false,
///     "items": [
///        {
///           "create": {
///              "_index": "test",
///              "_id": "1",
///              "_version": 1,
///              "result": "created",
///              "_shards": {
///                 "total": 2,
///                 "successful": 1,
///                 "failed": 0
///              },
///              "status": 201,
///              "_seq_no" : 0,
///              "_primary_term": 1
///           }
///        }
///     ]
/// }
/// ```
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilteredBulkCreateResponse {
    pub items: Vec<FailedCreateResult>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FailedCreateResult {
    pub create: CreateError,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateError {
    pub error: ErrorDetails,
}

#[derive(Debug, Deserialize)]
pub struct ErrorDetails {
    pub r#type: String,
    pub reason: String,
    pub index: String,
    pub index_uuid: String,
    pub shard: String,
}
