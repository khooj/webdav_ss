use super::{PropFuture, PropResult, PropStorage};
use crate::backend::normalized_path::NormalizedPath;
use futures_util::FutureExt;
use hyper::StatusCode;
use kv::*;
use tracing::{instrument, span, Instrument, Level};
use webdav_handler::fs::{DavProp, FsError};

#[derive(Clone)]
pub struct Kv {
    store: Store,
}
static_assertions::assert_impl_all!(Kv: Send, Sync, Clone);

#[derive(serde::Serialize, serde::Deserialize)]
struct PropKey {
    name: String,
    prefix: Option<String>,
    namespace: Option<String>,
}

impl PropKey {
    fn from_davprop(prop: &DavProp) -> Self {
        Self {
            name: prop.name.clone(),
            prefix: prop.prefix.clone(),
            namespace: prop.namespace.clone(),
        }
    }

    fn as_key(&self) -> Vec<u8> {
        bincode::serialize(&self).expect("can't serialize propkey")
    }
}

impl Kv {
    pub fn new(path: &str) -> Box<dyn PropStorage> {
        let cfg = Config::new(path).flush_every_ms(200).use_compression(true);
        let store = Store::new(cfg).expect("can't create store");
        Box::new(Kv { store }) as Box<dyn PropStorage>
    }

    fn get_values_bucket(&self) -> PropResult<Bucket<Vec<u8>, Vec<u8>>> {
        self.store
            .bucket(Some("values"))
            .map_err(|e| FsError::GeneralFailure)
    }

    fn get_existence_bucket(&self) -> PropResult<Bucket<&str, Vec<u8>>> {
        self.store
            .bucket(Some("existence"))
            .map_err(|e| FsError::GeneralFailure)
    }

    #[instrument(err, skip(self))]
    fn get_davprop(&self, path: &NormalizedPath) -> Result<DavProp, FsError> {
        let ex_bucket = self.get_existence_bucket()?;
        let bucket = self.get_values_bucket()?;

        let k = ex_bucket
            .get(&path.as_ref())
            .map_err(|e| FsError::NotFound)?;
        if k.is_none() {
            return Err(FsError::NotFound);
        }

        let k = k.unwrap();
        let k: PropKey = bincode::deserialize(&k).map_err(|e| FsError::NotFound)?;
        let v = bucket.get(&k.as_key()).map_err(|e| FsError::NotFound)?;
        if v.is_none() {
            return Err(FsError::NotFound);
        }
        let v = v.unwrap();
        let v: Option<Vec<u8>> = bincode::deserialize(&v).map_err(|e| FsError::NotFound)?;
        Ok(DavProp {
            name: k.name,
            namespace: k.namespace,
            prefix: k.prefix,
            xml: v,
        })
    }
}

impl PropStorage for Kv {
    fn patch_prop<'a>(
        &'a self,
        path: &'a NormalizedPath,
        (set, prop): (bool, DavProp),
    ) -> PropFuture<PropResult<(hyper::StatusCode, DavProp)>> {
        let span = span!(Level::DEBUG, "Kv::patch_prop");
        async move {
            let bucket = self.get_values_bucket()?;
            let ex_bucket = self.get_existence_bucket()?;

            let k = PropKey::from_davprop(&prop);
            if set {
                let v = bincode::serialize(&prop.xml).map_err(|e| FsError::GeneralFailure)?;
                bucket
                    .set(&k.as_key(), &v)
                    .map_err(|e| FsError::GeneralFailure)?;
                ex_bucket
                    .set(&path.as_str(), &k.as_key())
                    .map_err(|e| FsError::GeneralFailure)?;
            } else {
                bucket
                    .remove(&k.as_key())
                    .map_err(|e| FsError::GeneralFailure)?;
                ex_bucket
                    .remove(&path.as_str())
                    .map_err(|e| FsError::GeneralFailure)?;
            }
            Ok((StatusCode::OK, prop))
        }
        .instrument(span)
        .boxed()
    }

    fn have_props<'a>(&'a self, path: &'a NormalizedPath) -> PropFuture<bool> {
        async move {
            if path.is_collection() {
                return true;
            }

            let ex_bucket = self
                .get_existence_bucket()
                .expect("can't get existence bucket");
            ex_bucket.get(&path.as_str()).is_ok()
        }
        .boxed()
    }

    fn get_prop<'a>(
        &'a self,
        path: &'a NormalizedPath,
        prop: DavProp,
    ) -> PropFuture<PropResult<Vec<u8>>> {
        async move {
            let p = self.get_davprop(path)?;
            p.xml.ok_or(FsError::NotFound)
        }
        .boxed()
    }

    fn get_props<'a>(
        &'a self,
        path: &'a NormalizedPath,
        do_content: bool,
    ) -> PropFuture<PropResult<Vec<DavProp>>> {
        async move {
            let ex_bucket = self.get_existence_bucket()?;
            let bucket = self.get_values_bucket()?;
            let mut result = vec![];
            for it in ex_bucket.iter() {
                if it.is_err() {
                    continue;
                }

                let it = it.unwrap();
                if !it.key::<&str>().unwrap().contains(&path.as_str()) {
                    continue;
                }
                let k = it.value().unwrap();
                let v = bucket.get(&k).map_err(|e| FsError::GeneralFailure)?;
                if v.is_none() {
                    continue;
                }
                let k: PropKey = bincode::deserialize(&k).map_err(|e| FsError::GeneralFailure)?;
                let v: Option<Vec<u8>> =
                    bincode::deserialize(&v.unwrap()).map_err(|e| FsError::GeneralFailure)?;
                result.push(DavProp {
                    name: k.name,
                    namespace: k.namespace,
                    prefix: k.prefix,
                    xml: v,
                });
            }
            Ok(result)
        }
        .boxed()
    }

    fn remove_file<'a>(&'a self, path: &'a NormalizedPath) -> PropFuture<PropResult<()>> {
        async move {
            let ex_bucket = self.get_existence_bucket()?;
            let bucket = self.get_values_bucket()?;

            if let Ok(Some(v)) = ex_bucket.get(&path.as_ref()) {
                bucket.remove(&v).expect("can't remove file in remove_file");
                ex_bucket
                    .remove(&path.as_ref())
                    .expect("can't remove exist in remove_file");
            }
            Ok(())
        }
        .boxed()
    }

    fn remove_dir<'a>(&'a self, path: &'a NormalizedPath) -> PropFuture<PropResult<()>> {
        async move {
            let ex_bucket = self.get_existence_bucket()?;
            let bucket = self.get_values_bucket()?;
            for it in ex_bucket.iter() {
                if it.is_err() {
                    continue;
                }

                let it = it.unwrap();
                if !it.key::<&str>().unwrap().contains(&path.as_str()) {
                    continue;
                }
                let k = it.value().unwrap();
                let v = bucket.get(&k).map_err(|e| FsError::GeneralFailure)?;
                if v.is_none() {
                    continue;
                }

                bucket.remove(&k).expect("can't remove file in remove_dir");
                let k = it.key().unwrap();
                ex_bucket
                    .remove(&k)
                    .expect("can't remove file exist in remove_dir");
            }
            Ok(())
        }
        .boxed()
    }

    fn rename<'a>(
        &'a self,
        from: &'a NormalizedPath,
        to: &'a NormalizedPath,
    ) -> PropFuture<PropResult<()>> {
        async move {
            let oldprop = self.get_davprop(&from)?;
            self.remove_file(&from).await?;
            self.patch_prop(to, (true, oldprop)).await?;
            Ok(())
        }
        .boxed()
    }

    fn copy<'a>(
        &'a self,
        from: &'a NormalizedPath,
        to: &'a NormalizedPath,
    ) -> PropFuture<PropResult<()>> {
        let span = span!(Level::DEBUG, "Kv::copy");
        async move {
            let oldprop = self.get_davprop(from)?;
            self.patch_prop(to, (true, oldprop)).await?;
            Ok(())
        }
        .instrument(span)
        .boxed()
    }
}
