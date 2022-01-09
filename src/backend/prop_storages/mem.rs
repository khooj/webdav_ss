use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use super::{PropFuture, PropResult, PropStorage};
use crate::backend::normalized_path::NormalizedPath;
use futures_util::FutureExt;
use hyper::StatusCode;
use std::cell::RefCell;
use tracing::{debug, span, Instrument, Level};
use webdav_handler::fs::{DavProp, FsError};

#[derive(Clone)]
pub struct Memory {
    data: Arc<Mutex<RefCell<HashMap<String, DavProp>>>>,
}

impl Memory {
    pub fn new() -> Box<dyn PropStorage> {
        Box::new(Memory::new_unboxed()) as Box<dyn PropStorage>
    }

    pub fn new_unboxed() -> Memory {
        Memory {
            data: Arc::new(Mutex::new(RefCell::new(HashMap::new()))),
        }
    }

    pub fn get_all_props(&self) -> HashMap<String, DavProp> {
        let g = self.data.lock().unwrap();
        let b = g.borrow();
        b.clone()
    }

    fn get_prop_string(path: &NormalizedPath, prop: &DavProp) -> String {
        let ns = prop.namespace.clone().unwrap_or("".into());
        format!("{}.{}.{}", path.as_ref(), ns, prop.name)
    }

    pub fn add_prop(
        &self,
        path: &NormalizedPath,
        (set, prop): (bool, DavProp),
    ) -> PropResult<(StatusCode, DavProp)> {
        let data = self.data.lock().unwrap();
        let k = Memory::get_prop_string(path, &prop);
        let mut p_c = prop.clone();
        p_c.xml = None;
        if set {
            let mut b = data.borrow_mut();
            let v = b.entry(k.clone()).or_insert(prop.clone());
            *v = prop;
        } else {
            data.borrow_mut().remove(&k);
        }

        Ok((StatusCode::OK, p_c))
    }
}

impl PropStorage for Memory {
    fn have_props<'a>(&'a self, path: &'a NormalizedPath) -> PropFuture<bool> {
        let span = span!(Level::INFO, "Memory::have_props");
        async move {
            let g = self.data.lock().unwrap();
            let b = g.borrow();
            for k in b.keys() {
                if k.starts_with(path.as_ref()) {
                    debug!(contains = true, path = %path);
                    return true;
                }
            }
            debug!(contains = false, path = %path);
            false
        }
        .instrument(span)
        .boxed()
    }

    fn patch_prop<'a>(
        &'a self,
        path: &'a NormalizedPath,
        (set, prop): (bool, DavProp),
    ) -> PropFuture<PropResult<(StatusCode, DavProp)>> {
        async move { self.add_prop(path, (set, prop)) }.boxed()
    }

    fn get_prop<'a>(
        &'a self,
        path: &'a NormalizedPath,
        prop: DavProp,
    ) -> PropFuture<PropResult<Vec<u8>>> {
        let span = span!(Level::INFO, "Memory::get_prop");
        async move {
            let data = self.data.lock().unwrap();
            let k = Memory::get_prop_string(path, &prop);
            let r = data
                .borrow()
                .get(&k)
                .ok_or(FsError::NotFound)
                .and_then(|e| e.xml.clone().ok_or(FsError::NotFound));
            debug!(path = %path, result = ?r, prop = ?prop);
            r
        }
        .instrument(span)
        .boxed()
    }

    fn get_props<'a>(
        &'a self,
        path: &'a NormalizedPath,
        do_content: bool,
    ) -> PropFuture<PropResult<Vec<DavProp>>> {
        let span = span!(Level::INFO, "Memory::get_props");
        async move {
            let data = self.data.lock().unwrap();
            let mut r = vec![];
            let b = data.borrow();
            for k in b.keys() {
                if k.starts_with(path.as_ref()) {
                    let mut v = b.get(k).unwrap().clone();
                    if !do_content {
                        v.xml = None;
                    }
                    r.push(v);
                }
            }
            debug!(path = %path, result = ?r, do_content = do_content);

            Ok(r)
        }
        .instrument(span)
        .boxed()
    }

    fn remove_file<'a>(&'a self, path: &'a NormalizedPath) -> PropFuture<PropResult<()>> {
        let span = span!(Level::INFO, "Memory::remove_file");
        async move {
            let data = self.data.lock().unwrap();
            let mut b = data.borrow_mut();
            let mut rm = None;
            for k in b.keys() {
                if k.starts_with(path.as_ref()) {
                    rm = Some(k.clone());
                    break;
                }
            }

            debug!(path = %path);
            if rm.is_none() {
                Ok(())
            } else {
                b.remove(&rm.unwrap());
                Ok(())
            }
        }
        .instrument(span)
        .boxed()
    }

    fn remove_dir<'a>(&'a self, path: &'a NormalizedPath) -> PropFuture<PropResult<()>> {
        let span = span!(Level::INFO, "Memory::remove_dir");
        async move {
            let data = self.data.lock().unwrap();
            let mut b = data.borrow_mut();
            let mut rm = None;
            for k in b.keys() {
                if k.starts_with(path.as_ref()) {
                    rm = Some(k.clone());
                    break;
                }
            }

            debug!(path = %path);
            if rm.is_none() {
                Ok(())
            } else {
                b.remove(&rm.unwrap());
                Ok(())
            }
        }
        .instrument(span)
        .boxed()
    }

    fn rename<'a>(
        &'a self,
        from: &'a NormalizedPath,
        to: &'a NormalizedPath,
    ) -> PropFuture<PropResult<()>> {
        let span = span!(Level::INFO, "Memory::rename");
        async move {
            let data = self.data.lock().unwrap();
            let mut b = data.borrow_mut();
            let mut rn = vec![];
            for k in b.keys() {
                if k.starts_with(from.as_ref()) {
                    let pp: NormalizedPath = k.strip_prefix(from.as_ref()).unwrap().into();
                    let pp = format!("{}{}", to.as_ref(), pp.as_ref());
                    rn.push((k.clone(), pp));
                }
            }

            for (k, pp) in rn {
                debug!(from = %k, to = %pp);
                let prop = b.remove(&k).unwrap();
                b.entry(pp.to_string()).or_insert(prop);
            }
            Ok(())
        }
        .instrument(span)
        .boxed()
    }

    fn copy<'a>(
        &'a self,
        from: &'a NormalizedPath,
        to: &'a NormalizedPath,
    ) -> PropFuture<PropResult<()>> {
        let span = span!(Level::INFO, "Memory::copy");
        async move {
            let data = self.data.lock().unwrap();
            let mut b = data.borrow_mut();
            let mut cp = vec![];
            for k in b.keys() {
                if k.starts_with(from.as_ref()) {
                    let pp: NormalizedPath = k.strip_prefix(from.as_ref()).unwrap().into();
                    let pp = format!("{}{}", to.as_ref(), pp.as_ref());
                    cp.push((k.clone(), pp.to_string()));
                }
            }

            for (k, pp) in cp {
                debug!(from = %k, to = %pp);
                let prop = b.get(&k).unwrap().clone();
                b.entry(pp.to_string()).or_insert(prop);
            }
            Ok(())
        }
        .instrument(span)
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rename() -> anyhow::Result<()> {
        let mem = Memory::new();
        let prop = DavProp {
            name: "name1".into(),
            namespace: Some("namespace1".into()),
            prefix: None,
            xml: Some([1, 2, 3].into()),
        };

        mem.patch_prop(&"/fs3/some/prop".into(), (true, prop.clone()))
            .await?;

        mem.rename(&"/fs3/some/prop".into(), &"/fs3/some/prop2".into())
            .await?;

        let p = mem
            .get_prop(&"/fs3/some/prop2".into(), prop.clone())
            .await?;

        assert_eq!(p, prop.xml.unwrap());

        Ok(())
    }
}
