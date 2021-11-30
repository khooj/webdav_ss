use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use super::{PropFuture, PropResult, PropStorage};
use crate::backend::normalized_path::NormalizedPath;
use anyhow::anyhow;
use futures_util::FutureExt;
use hyper::StatusCode;
use std::cell::RefCell;
use webdav_handler::fs::DavProp;

#[derive(Clone)]
pub struct Memory {
    data: Arc<Mutex<RefCell<HashMap<String, DavProp>>>>,
    have_prop: Arc<Mutex<RefCell<HashSet<String>>>>,
}

impl Memory {
    pub fn new() -> Self {
        Memory {
            data: Arc::new(Mutex::new(RefCell::new(HashMap::new()))),
            have_prop: Arc::new(Mutex::new(RefCell::new(HashSet::new()))),
        }
    }

    fn get_prop_string(path: &NormalizedPath, prop: &DavProp) -> String {
        let ns = prop.namespace.clone().unwrap_or("".into());
        let prefix = prop.prefix.clone().unwrap_or("".into());
        format!("{}.{}.{}.{}", path.as_ref(), ns, prefix, prop.name)
    }
}

impl PropStorage for Memory {
    fn have_props<'a>(&'a self, path: &'a NormalizedPath) -> PropFuture<bool> {
        async move {
            let g = self.have_prop.lock().unwrap();
            let r = g.borrow().contains(&path.to_string());
            r
        }
        .boxed()
    }

    fn patch_props<'a>(
        &'a self,
        path: &'a NormalizedPath,
        patch: Vec<(bool, DavProp)>,
    ) -> PropFuture<PropResult<Vec<(StatusCode, DavProp)>>> {
        async move {
            let have_prop = self.have_prop.lock().unwrap();
            let data = self.data.lock().unwrap();
            let mut r = vec![];
            for (set, prop) in patch {
                let k = Memory::get_prop_string(path, &prop);
                if set {
                    let mut b = data.borrow_mut();
                    let v = b.entry(k.clone()).or_insert(prop.clone());
                    *v = prop.clone();
                    have_prop.borrow_mut().insert(k);
                } else {
                    data.borrow_mut().remove(&k);
                    have_prop.borrow_mut().remove(&k);
                }
                r.push((StatusCode::OK, prop));
            }

            Ok(r)
        }
        .boxed()
    }

    fn get_prop<'a>(
        &'a self,
        path: &'a NormalizedPath,
        prop: DavProp,
    ) -> PropFuture<PropResult<Vec<u8>>> {
        async move {
            let data = self.data.lock().unwrap();
            let k = Memory::get_prop_string(path, &prop);
            let r = data.borrow()
                .get(&k)
                .ok_or(anyhow!("prop not found"))
                .map(|e| e.xml.clone().unwrap_or(vec![]));
            r
        }
        .boxed()
    }

    fn get_props<'a>(
        &'a self,
        path: &'a NormalizedPath,
        _do_content: bool,
    ) -> PropFuture<PropResult<Vec<DavProp>>> {
        async move {
            let data = self.data.lock().unwrap();
            let mut r = vec![];
            for k in data.borrow().keys() {
                if k.starts_with(path.as_ref()) {
                    let v = data.borrow().get(k).unwrap().clone();
                    r.push(v);
                }
            }

            Ok(r)
        }
        .boxed()
    }
}
