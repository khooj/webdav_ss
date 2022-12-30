#[derive(YaDeserialize, Debug)]
#[yaserde(namespace = "http://s3.amazonaws.com/doc/2006-03-01/")]
pub struct InitiateMultipartUploadResponse {
    #[yaserde(rename = "Bucket")]
    bucket: String,
    #[yaserde(rename = "Key")]
    pub key: String,
    #[yaserde(rename = "UploadId")]
    pub upload_id: String,
}

/// Owner information for the object
#[derive(YaDeserialize, Debug, Clone)]
#[yaserde(namespace = "http://s3.amazonaws.com/doc/2006-03-01/")]
pub struct Owner {
    #[yaserde(rename = "DisplayName")]
    /// Object owner's name.
    pub display_name: String,
    #[yaserde(rename = "ID")]
    /// Object owner's ID.
    pub id: String,
}

/// An individual object in a `ListBucketResult`
#[derive(YaDeserialize, Debug, Clone)]
#[yaserde(namespace = "http://s3.amazonaws.com/doc/2006-03-01/")]
pub struct Object {
    #[yaserde(rename = "LastModified", child)]
    /// Date and time the object was last modified.
    pub last_modified: String,
    #[yaserde(rename = "ETag", child)]
    /// The entity tag is an MD5 hash of the object. The ETag only reflects changes to the
    /// contents of an object, not its metadata.
    pub e_tag: String,
    #[yaserde(rename = "StorageClass", child)]
    /// STANDARD | STANDARD_IA | REDUCED_REDUNDANCY | GLACIER
    pub storage_class: String,
    #[yaserde(rename = "Key", child)]
    /// The object's key
    pub key: String,
    #[yaserde(rename = "Owner")]
    /// Bucket owner
    pub owner: Option<Owner>,
    #[yaserde(rename = "Size", child)]
    /// Size in bytes of the object.
    pub size: u64,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Tagging {
    #[serde(rename = "TagSet")]
    pub tag_set: TagSet,
}

#[derive(Deserialize, Debug, Clone)]
pub struct TagSet {
    #[serde(rename = "Tag")]
    pub tags: Vec<Tag>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Tag {
    // #[serde(rename = "Tag")]
    // pub kvpair: KVPair,
    #[serde(rename = "Key")]
    pub key: String,
    #[serde(rename = "Value")]
    pub value: String,
}

impl Tag {
    pub fn key(&self) -> String {
        self.key.clone()
        // "".into()
    }

    pub fn value(&self) -> String {
        self.value.clone()
        // "".into()
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct KVPair {
    #[serde(rename = "Key")]
    pub key: String,
    #[serde(rename = "Value")]
    pub value: String,
}

use std::fmt;

use yaserde_derive::{YaDeserialize, YaSerialize};

impl fmt::Display for CompleteMultipartUploadData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = String::new();
        for part in self.parts.clone() {
            parts.push_str(&serde_xml_rs::to_string(&part).unwrap())
        }
        write!(
            f,
            "<CompleteMultipartUpload>{}</CompleteMultipartUpload>",
            parts
        )
    }
}

impl CompleteMultipartUploadData {
    pub fn len(&self) -> usize {
        self.to_string().as_bytes().len()
    }

    pub fn is_empty(&self) -> bool {
        self.to_string().as_bytes().len() == 0
    }
}

#[derive(Debug, Clone)]
pub struct CompleteMultipartUploadData {
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CompleteMultipartUploadResult {
    #[serde(rename = "Location")]
    pub location: String,
    #[serde(rename = "Bucket")]
    pub bucket: String,
    #[serde(rename = "Key")]
    pub key: String,
    #[serde(rename = "ETag")]
    pub etag: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Part {
    #[serde(rename = "PartNumber")]
    pub part_number: u32,
    #[serde(rename = "ETag")]
    pub etag: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct BucketLocationResult {
    #[serde(rename = "$value")]
    pub region: String,
}

/// The parsed result of a s3 bucket listing
#[derive(Debug, Clone, Default, YaDeserialize)]
#[yaserde(namespace = "http://s3.amazonaws.com/doc/2006-03-01/")]
pub struct ListBucketResult {
    #[yaserde(rename = "Name", child)]
    /// Name of the bucket.
    pub name: String,
    #[yaserde(rename = "NextMarker", child)]
    /// When the response is truncated (that is, the IsTruncated element value in the response
    /// is true), you can use the key name in this field as a marker in the subsequent request
    /// to get next set of objects. Amazon S3 lists objects in UTF-8 character encoding in
    /// lexicographical order.
    pub next_marker: Option<String>,
    #[yaserde(rename = "Delimiter", child)]
    /// A delimiter is a character you use to group keys.
    pub delimiter: Option<String>,
    #[yaserde(rename = "MaxKeys", child)]
    /// Sets the maximum number of keys returned in the response body.
    pub max_keys: i32,
    #[yaserde(rename = "Prefix", child)]
    /// Limits the response to keys that begin with the specified prefix.
    pub prefix: String,
    #[yaserde(rename = "Marker", child)]
    /// Indicates where in the bucket listing begins. Marker is included in the response if
    /// it was sent with the request.
    pub marker: Option<String>,
    #[yaserde(rename = "EncodingType", child)]
    /// Specifies the encoding method to used
    pub encoding_type: Option<String>,
    #[yaserde(
        rename = "IsTruncated", child,
        // deserialize_with = "super::deserializer::bool_deserializer"
    )]
    ///  Specifies whether (true) or not (false) all of the results were returned.
    ///  If the number of results exceeds that specified by MaxKeys, all of the results
    ///  might not be returned.
    pub is_truncated: bool,
    #[yaserde(rename = "NextContinuationToken", default, child)]
    pub next_continuation_token: Option<String>,
    #[yaserde(rename = "Contents", default)]
    // / Metadata about each object returned.
    pub contents: Vec<Object>,
    #[yaserde(rename = "CommonPrefixes", default)]
    // / All of the keys rolled up into a common prefix count as a single return when
    // / calculating the number of returns.
    pub common_prefixes: Vec<CommonPrefix>,
}

/// Workaround enum for ListBucketResult
// #[derive(Deserialize, Debug, Clone)]
// pub enum ListBucketVecData {
//     Name(String),
//     NextMarker(Option<String>),
//     Delimiter(Option<String>),
//     MaxKeys(i32),
//     Prefix(String),
//     Marker(Option<String>),
//     EncodingType(Option<String>),
//     IsTruncated(#[serde(deserialize_with = "super::deserializer::bool_deserializer")] bool),
//     NextContinuationToken(Option<String>),
//     Contents(Object),
//     CommonPrefixes(Option<CommonPrefix>),
//     #[serde(other)]
//     Other,
// }

// #[derive(Deserialize, Clone, Debug)]
// pub struct ListBucketResultProxy {
//     #[serde(rename = "$value")]
//     pub vec_data: Vec<ListBucketVecData>,
// }

/// `CommonPrefix` is used to group keys
#[derive(YaDeserialize, Debug, Clone)]
#[yaserde(namespace = "http://s3.amazonaws.com/doc/2006-03-01/")]
pub struct CommonPrefix {
    #[yaserde(rename = "Prefix", child)]
    /// Keys that begin with the indicated prefix.
    pub prefix: String,
}

// Taken from https://github.com/rusoto/rusoto
#[derive(Deserialize, Debug, Default, Clone)]
pub struct HeadObjectResult {
    #[serde(rename = "AcceptRanges")]
    /// Indicates that a range of bytes was specified.
    pub accept_ranges: Option<String>,
    #[serde(rename = "CacheControl")]
    /// Specifies caching behavior along the request/reply chain.
    pub cache_control: Option<String>,
    #[serde(rename = "ContentDisposition")]
    /// Specifies presentational information for the object.
    pub content_disposition: Option<String>,
    #[serde(rename = "ContentEncoding")]
    /// Specifies what content encodings have been applied to the object and thus what decoding mechanisms must be applied to obtain the media-type referenced by the Content-Type header field.
    pub content_encoding: Option<String>,
    #[serde(rename = "ContentLanguage")]
    /// The language the content is in.
    pub content_language: Option<String>,
    #[serde(rename = "ContentLength")]
    /// Size of the body in bytes.
    pub content_length: Option<i64>,
    #[serde(rename = "ContentType")]
    /// A standard MIME type describing the format of the object data.
    pub content_type: Option<String>,
    #[serde(rename = "DeleteMarker")]
    /// Specifies whether the object retrieved was (true) or was not (false) a Delete Marker.
    pub delete_marker: Option<bool>,
    #[serde(rename = "ETag")]
    /// An ETag is an opaque identifier assigned by a web server to a specific version of a resource found at a URL.
    pub e_tag: Option<String>,
    #[serde(rename = "Expiration")]
    /// If the object expiration is configured, the response includes this header. It includes the expiry-date and rule-id key-value pairs providing object expiration information.
    /// The value of the rule-id is URL encoded.
    pub expiration: Option<String>,
    #[serde(rename = "Expires")]
    /// The date and time at which the object is no longer cacheable.
    pub expires: Option<String>,
    #[serde(rename = "LastModified")]
    /// Last modified date of the object
    pub last_modified: Option<String>,
    #[serde(rename = "Metadata", default)]
    /// A map of metadata to store with the object in S3.
    pub metadata: Option<::std::collections::HashMap<String, String>>,
    #[serde(rename = "MissingMeta")]
    /// This is set to the number of metadata entries not returned in x-amz-meta headers. This can happen if you create metadata using an API like SOAP that supports more flexible metadata than
    /// the REST API. For example, using SOAP, you can create metadata whose values are not legal HTTP headers.
    pub missing_meta: Option<i64>,
    #[serde(rename = "ObjectLockLegalHoldStatus")]
    /// Specifies whether a legal hold is in effect for this object. This header is only returned if the requester has the s3:GetObjectLegalHold permission.
    /// This header is not returned if the specified version of this object has never had a legal hold applied.
    pub object_lock_legal_hold_status: Option<String>,
    #[serde(rename = "ObjectLockMode")]
    /// The Object Lock mode, if any, that's in effect for this object.
    pub object_lock_mode: Option<String>,
    #[serde(rename = "ObjectLockRetainUntilDate")]
    /// The date and time when the Object Lock retention period expires.
    /// This header is only returned if the requester has the s3:GetObjectRetention permission.
    pub object_lock_retain_until_date: Option<String>,
    #[serde(rename = "PartsCount")]
    /// The count of parts this object has.
    pub parts_count: Option<i64>,
    #[serde(rename = "ReplicationStatus")]
    /// If your request involves a bucket that is either a source or destination in a replication rule.
    pub replication_status: Option<String>,
    #[serde(rename = "RequestCharged")]
    pub request_charged: Option<String>,
    #[serde(rename = "Restore")]
    /// If the object is an archived object (an object whose storage class is GLACIER), the response includes this header if either the archive restoration is in progress or an archive copy is already restored.
    /// If an archive copy is already restored, the header value indicates when Amazon S3 is scheduled to delete the object copy.
    pub restore: Option<String>,
    #[serde(rename = "SseCustomerAlgorithm")]
    /// If server-side encryption with a customer-provided encryption key was requested, the response will include this header confirming the encryption algorithm used.
    pub sse_customer_algorithm: Option<String>,
    #[serde(rename = "SseCustomerKeyMd5")]
    /// If server-side encryption with a customer-provided encryption key was requested, the response will include this header to provide round-trip message integrity verification of the customer-provided encryption key.
    pub sse_customer_key_md5: Option<String>,
    #[serde(rename = "SsekmsKeyId")]
    /// If present, specifies the ID of the AWS Key Management Service (AWS KMS) symmetric customer managed customer master key (CMK) that was used for the object.
    pub ssekms_key_id: Option<String>,
    #[serde(rename = "ServerSideEncryption")]
    /// If the object is stored using server-side encryption either with an AWS KMS customer master key (CMK) or an Amazon S3-managed encryption key,
    /// The response includes this header with the value of the server-side encryption algorithm used when storing this object in Amazon S3 (for example, AES256, aws:kms).
    pub server_side_encryption: Option<String>,
    #[serde(rename = "StorageClass")]
    /// Provides storage class information of the object. Amazon S3 returns this header for all objects except for S3 Standard storage class objects.
    pub storage_class: Option<String>,
    #[serde(rename = "VersionId")]
    /// Version of the object.
    pub version_id: Option<String>,
    #[serde(rename = "WebsiteRedirectLocation")]
    /// If the bucket is configured as a website, redirects requests for this object to another object in the same bucket or to an external URL. Amazon S3 stores the value of this header in the object metadata.
    pub website_redirect_location: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct CopyObjectResult {
    #[serde(rename = "ETag")]
    pub etag: String,
    #[serde(rename = "LastModified")]
    pub last_modified: String,
}

#[derive(Deserialize, Debug)]
pub struct AwsError {
    #[serde(rename = "Code")]
    pub code: String,
    #[serde(rename = "Message")]
    pub message: String,
    #[serde(rename = "RequestId")]
    pub request_id: String,
}

#[cfg(test)]
mod tests {
    use super::{KVPair, ListBucketResult, Tag, Tagging};
    use serde_xml_rs;

    #[test]
    fn check_tagging() {
        let input = r##"
<Tagging>
	<TagSet>
		<Tag>
			<Key>modified</Key>
			<Value>1633124094</Value>
		</Tag>
		<Tag>
			<Key>status_changed</Key>
			<Value>1633124094</Value>
		</Tag>
		<Tag>
			<Key>accessed</Key>
			<Value>1633124094</Value>
		</Tag>
		<Tag>
			<Key>created</Key>
			<Value>1633124094</Value>
		</Tag>
	</TagSet>
</Tagging>
        "##;
        let res: Tagging = serde_xml_rs::from_reader(input.as_bytes()).unwrap();
        assert_eq!(res.tag_set.tags.len(), 4);
    }

    #[test]
    fn check_tagging2() {
        let input = r##"
		<Tag Key="modified" Value="1633124094" />
		<Tag Key="created" Value="1633124094" />
		<Tag Key="accessed" Value="1633124094" />
		<Tag Key="status_changed" Value="1633124094" />
        "##;
        let res: Vec<Tag> = serde_xml_rs::from_reader(input.as_bytes()).unwrap();
        assert_eq!(res.len(), 4);
    }

    #[test]
    fn check_listbucket() {
        let input = r##"
        <?xml version= "1.0" encoding= "utf-8"?>
<ListBucketResult xmlns= "http://s3.amazonaws.com/doc/2006-03-01/">
	<Prefix>mobile/</Prefix>
	<IsTruncated>false</IsTruncated>
	<Delimiter>/</Delimiter>
	<MaxKeys>1000</MaxKeys>
	<KeyCount>27</KeyCount>
	<Name>bucketname</Name>
	<CommonPrefixes>
		<Prefix>mobile/Alarms/</Prefix>
	</CommonPrefixes>
    <Contents>
        <LastModified>somestring</LastModified>
        <ETag>etag</ETag>
        <StorageClass>STANDARD</StorageClass>
        <Key>key</Key>
        <Size>12313</Size>
    </Contents>
	<CommonPrefixes>
		<Prefix>mobile/Android/</Prefix>
	</CommonPrefixes>
	<CommonPrefixes>
		<Prefix>mobile/Movies/</Prefix>
	</CommonPrefixes>
    <Contents>
        <LastModified>somestring</LastModified>
        <ETag>etag</ETag>
        <StorageClass>STANDARD</StorageClass>
        <Key>key</Key>
        <Size>12313</Size>
    </Contents>
	<CommonPrefixes>
		<Prefix>mobile123</Prefix>
	</CommonPrefixes>
</ListBucketResult>
        "##;

        // let res: ListBucketResultProxy = serde_xml_rs::from_reader(input.as_bytes()).unwrap();
        let res: ListBucketResult = yaserde::de::from_str(input).unwrap();

        let input2 = r##"
<?xml version= "1.0" encoding= "UTF-8"?>
<ListBucketResult xmlns= "http://s3.amazonaws.com/doc/2006-03-01/">
	<Name>test</Name>
	<Prefix>litmus/movecoll/</Prefix>
	<KeyCount>1</KeyCount>
	<MaxKeys>1000</MaxKeys>
	<Delimiter>/</Delimiter>
	<IsTruncated>false</IsTruncated>
	<Contents>
		<Key>litmus/movecoll/.dir</Key>
		<LastModified>2022-10-09T12:58:27.084Z</LastModified>
		<ETag>&#34;d41d8cd98f00b204e9800998ecf8427e&#34;</ETag>
		<Size>0</Size>
		<Owner>
			<ID>02d6176db174dc93cb1b899f7c6078f08654445fe8cf1b6ce98d8855f66bdbf4</ID>
			<DisplayName>minio</DisplayName>
		</Owner>
		<StorageClass>STANDARD</StorageClass>
	</Contents>
</ListBucketResult>
        "##;
        let res: ListBucketResult = yaserde::de::from_str(input2).unwrap();
    }
}
