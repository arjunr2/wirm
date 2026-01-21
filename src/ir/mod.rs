//! The Intermediate Representation for components and modules.

pub mod component;
pub mod function;
mod helpers;
pub mod id;
#[cfg(test)]
pub mod instr_tests;
pub mod module;
pub mod types;
pub(crate) mod wrappers;

#[derive(Clone, Debug)]
pub struct AppendOnlyVec<T> {
    vec: Vec<T>,
}
impl<T> Default for AppendOnlyVec<T> {
    fn default() -> Self {
        Self { vec: Vec::new() }
    }
}
impl<T> AppendOnlyVec<T> {
    pub fn len(&self) -> usize {
        self.vec.len()
    }
    pub fn is_empty(&self) -> bool {
        self.vec.is_empty()
    }

    // INSERTs (only accessible in the crate)
    /// To push an item into the vector. Note that this is not exposed beyond the crate.
    /// This is to protect users from appending IR nodes without them going through
    /// correct preprocessing (scoping, registration, ID assignment).
    pub(crate) fn push(&mut self, value: T) {
        self.vec.push(value);
    }
    /// To append a list of items into the vector. Note that this is not exposed beyond the crate.
    /// This is to protect users from appending IR nodes without them going through
    /// correct preprocessing (scoping, registration, ID assignment).
    pub(crate) fn append(&mut self, other: &mut Vec<T>) {
        self.vec.append(other);
    }

    // GETs
    pub fn maybe_get(&self, i: usize) -> Option<&T> {
        self.vec.get(i)
    }
    pub fn get(&self, i: usize) -> &T {
        &self.vec[i]
    }
    pub fn get_mut(&mut self, i: usize) -> &mut T {
        &mut self.vec[i]
    }
    pub fn last(&mut self) -> Option<&T> {
        self.vec.last()
    }

    // ITERation
    pub fn iter(&'_ self) -> core::slice::Iter<'_, T> {
        self.vec.iter()
    }
    pub fn iter_mut(&'_ mut self) -> core::slice::IterMut<'_, T> {
        self.vec.iter_mut()
    }
    pub fn slice_from(&self, start: usize) -> &[T] {
        &self.vec[start..]
    }
    pub fn slice_from_mut(&mut self, start: usize) -> &mut [T] {
        &mut self.vec[start..]
    }

    /// We will only ever expose a non-mutable vec here!
    /// Any mutation can only be appending or edit-in-place.
    /// Exposing a mutable vec would allow illegal operations.
    pub fn as_vec(&self) -> &Vec<T> {
        &self.vec
    }

    // no remove, no replace
}
