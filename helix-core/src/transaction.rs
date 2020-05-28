// pub struct Change {
//     from: usize,
//     to: usize,
//     insert: Option<String>,
// }

// 40 bytes (8 + 24 + 8) -> strings are really big 24 as String, 16 as &str
// pub struct Change {
//     /// old extent
//     old_extent: usize,
//     /// inserted text, new extent equal to insert length
//     insert: Option<String>,
//     /// distance from the previous change
//     distance: usize,
// }

use crate::{Buffer, Rope, Selection, Tendril};

// TODO: divided into three different operations, I sort of like having just
// Splice { extent, Option<text>, distance } better.
// insert: Splice { extent: 0, text: Some("a"), distance: 2 }
// delete: Splice { extent: 2, text: None, distance: 2 }
// replace: Splice { extent: 2, text: Some("abc"), distance: 2 }
// unchanged?: Splice { extent: 0, text: None, distance: 2 }
// harder to compose though.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Move cursor by n characters.
    Retain(usize),
    /// Delete n characters.
    Delete(usize),
    /// Insert text at position.
    Insert(Tendril),
}

impl Change {
    pub fn new(from: usize, to: usize, insert: Option<Tendril>) {
        // old_extent, new_extent, insert
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Assoc {
    Before,
    After,
}

// ChangeSpec = Change | ChangeSet | Vec<Change>
// ChangeDesc as a ChangeSet without text: can't be applied, cheaper to store.
// ChangeSet = ChangeDesc with Text
#[derive(Debug)]
pub struct ChangeSet {
    // basically Vec<ChangeDesc> where ChangeDesc = (current len, replacement len?)
    // (0, n>0) for insertion, (n>0, 0) for deletion, (>0, >0) for replacement
    // sections: Vec<(usize, isize)>,
    changes: Vec<Change>,
    /// The required document length. Will refuse to apply changes unless it matches.
    len: usize,
}

impl ChangeSet {
    #[must_use]
    pub fn new(buf: &Buffer) -> Self {
        let len = buf.contents.len_chars();
        Self {
            changes: vec![Change::Retain(len)],
            len,
        }
    }

    // TODO: from iter

    /// Combine two changesets together.
    /// In other words,  If `this` goes `docA` → `docB` and `other` represents `docB` → `docC`, the
    /// returned value will represent the change `docA` → `docC`.
    pub fn compose(self, other: ChangeSet) -> Result<Self, ()> {
        if self.len != other.len {
            // length mismatch
            return Err(());
        }

        let len = self.changes.len();

        let mut changes_a = self.changes.into_iter();
        let mut changes_b = other.changes.into_iter();

        let mut head_a = changes_a.next();
        let mut head_b = changes_b.next();

        let mut changes: Vec<Change> = Vec::with_capacity(len); // TODO: max(a, b), shrink_to_fit() afterwards

        loop {
            use std::cmp::Ordering;
            use Change::*;
            match (head_a, head_b) {
                // we are done
                (None, None) => {
                    break;
                }
                // deletion in A
                (Some(change @ Delete(..)), b) => {
                    changes.push(change);
                    head_a = changes_a.next();
                    head_b = b;
                }
                // insertion in B
                (a, Some(change @ Insert(..))) => {
                    changes.push(change);
                    head_a = a;
                    head_b = changes_b.next();
                }
                (None, _) | (_, None) => return Err(()),
                (Some(Retain(i)), Some(Retain(j))) => match i.cmp(&j) {
                    Ordering::Less => {
                        changes.push(Retain(i));
                        head_a = changes_a.next();
                        head_b = Some(Retain(j - i));
                    }
                    Ordering::Equal => {
                        changes.push(Retain(i));
                        head_a = changes_a.next();
                        head_b = changes_b.next();
                    }
                    Ordering::Greater => {
                        changes.push(Retain(j));
                        head_a = Some(Retain(i - j));
                        head_b = changes_b.next();
                    }
                },
                (Some(Insert(mut s)), Some(Delete(j))) => {
                    let len = s.chars().count();
                    match len.cmp(&j) {
                        Ordering::Less => {
                            head_a = changes_a.next();
                            head_b = Some(Delete(j - len));
                        }
                        Ordering::Equal => {
                            head_a = changes_a.next();
                            head_b = changes_b.next();
                        }
                        Ordering::Greater => {
                            // figure out the byte index of the truncated string end
                            let (pos, _) = s.char_indices().nth(len - j).unwrap();
                            // calculate the difference
                            let to_drop = s.len() - pos;
                            s.pop_back(to_drop as u32);
                            head_a = Some(Insert(s));
                            head_b = changes_b.next();
                        }
                    }
                }
                (Some(Insert(mut s)), Some(Retain(j))) => {
                    let len = s.chars().count();
                    match len.cmp(&j) {
                        Ordering::Less => {
                            changes.push(Insert(s));
                            head_a = changes_a.next();
                            head_b = Some(Retain(j - len));
                        }
                        Ordering::Equal => {
                            changes.push(Insert(s));
                            head_a = changes_a.next();
                            head_b = changes_b.next();
                        }
                        Ordering::Greater => {
                            // figure out the byte index of the truncated string end
                            let (pos, _) = s.char_indices().nth(j).unwrap();
                            // calculate the difference
                            let to_drop = s.len() - pos;
                            s.pop_back(to_drop as u32);
                            head_a = Some(Insert(s));
                            head_b = changes_b.next();
                        }
                    }
                }
                (Some(Retain(i)), Some(Delete(j))) => match i.cmp(&j) {
                    Ordering::Less => {
                        changes.push(Delete(i));
                        head_a = changes_a.next();
                        head_b = Some(Delete(j - i));
                    }
                    Ordering::Equal => {
                        changes.push(Delete(j));
                        head_a = changes_a.next();
                        head_b = changes_b.next();
                    }
                    Ordering::Greater => {
                        changes.push(Delete(j));
                        head_a = Some(Retain(i - j));
                        head_b = changes_b.next();
                    }
                },
            };
        }

        Ok(Self {
            len: self.len,
            changes,
        })
    }

    /// Given another change set starting in the same document, maps this
    /// change set over the other, producing a new change set that can be
    /// applied to the document produced by applying `other`. When
    /// `before` is `true`, order changes as if `this` comes before
    /// `other`, otherwise (the default) treat `other` as coming first.
    ///
    /// Given two changes `A` and `B`, `A.compose(B.map(A))` and
    /// `B.compose(A.map(B, true))` will produce the same document. This
    /// provides a basic form of [operational
    /// transformation](https://en.wikipedia.org/wiki/Operational_transformation),
    /// and can be used for collaborative editing.
    pub fn map(self, other: Self) -> Self {
        unimplemented!()
    }

    /// Returns a new changeset that reverts this one. Useful for `undo` implementation.
    pub fn invert(self) -> Self {
        unimplemented!()
    }

    pub fn apply(&self, text: &mut Rope) {
        // TODO: validate text.chars() == self.len

        let mut pos = 0;

        for change in &self.changes {
            use Change::*;
            match change {
                Retain(n) => {
                    pos += n;
                }
                Delete(n) => {
                    text.remove(pos..pos + *n);
                    // pos += n;
                }
                Insert(s) => {
                    text.insert(pos, s);
                    pos += s.len();
                }
            }
        }
    }

    /// `true` when the set is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        let len = self.changes.len();
        len == 0 || (len == 1 && self.changes[0] == Change::Retain(self.len))
    }

    /// Map a position through the changes.
    ///
    /// `assoc` indicates which size to associate the position with. `Before` will keep the
    /// position close to the character before, and will place it before insertions over that
    /// range, or at that point. `After` will move it forward, placing it at the end of such
    /// insertions.
    pub fn map_pos(&self, pos: usize, assoc: Assoc) -> usize {
        use Change::*;
        let mut old_pos = 0;
        let mut new_pos = 0;

        let mut iter = self.changes.iter().peekable();

        while let Some(change) = iter.next() {
            let len = match change {
                Delete(i) | Retain(i) => *i,
                Insert(_) => 0,
            };
            let old_end = old_pos + len;

            match change {
                Retain(_len) => {
                    if old_end > pos {
                        return new_pos + (pos - old_pos);
                    }
                    new_pos += len;
                }
                Delete(_len) => {
                    // a subsequent ins means a replace, consume it
                    let ins = if let Some(Insert(s)) = iter.peek() {
                        iter.next();
                        s.chars().count()
                    } else {
                        0
                    };

                    // in range
                    if old_end > pos {
                        // at point or tracking before
                        if pos == old_pos || assoc == Assoc::Before {
                            return new_pos;
                        } else {
                            // place to end of delete
                            return new_pos + ins;
                        }
                    }

                    new_pos += ins;
                }
                Insert(s) => {
                    let ins = s.chars().count();
                    // at insert point
                    if old_pos == pos {
                        // return position before inserted text
                        if assoc == Assoc::Before {
                            return new_pos;
                        } else {
                            // after text
                            return new_pos + ins;
                        }
                    }

                    new_pos += ins;
                }
            }
            old_pos = old_end;
        }

        if pos > old_pos {
            panic!(
                "Position {} is out of range for changeset len {}!",
                pos, old_pos
            )
        }
        new_pos
    }
}

// trait Transaction
// trait StrictTransaction

/// Transaction represents a single undoable unit of changes. Several changes can be grouped into
/// a single transaction.
pub struct Transaction {
    /// Changes made to the buffer.
    changes: ChangeSet,
    /// When set, explicitly updates the selection.
    selection: Option<Selection>,
    // effects, annotations
    // scroll_into_view
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn composition() {
        use Change::*;

        let a = ChangeSet {
            changes: vec![
                Retain(5),
                Insert("!".into()),
                Retain(1),
                Delete(2),
                Insert("ab".into()),
            ],
            len: 7,
        };

        let b = ChangeSet {
            changes: vec![Delete(5), Insert("world".into()), Retain(4)],
            len: 7,
        };

        let mut text = Rope::from("hello xz");

        // should probably return cloned text
        a.compose(b).unwrap().apply(&mut text);

        // unimplemented!("{:?}", text);
        // TODO: assert
    }

    #[test]
    fn map_pos() {
        use Change::*;

        // maps inserts
        let cs = ChangeSet {
            changes: vec![Retain(4), Insert("!!".into()), Retain(4)],
            len: 8,
        };

        assert_eq!(cs.map_pos(0, Assoc::Before), 0); // before insert region
        assert_eq!(cs.map_pos(4, Assoc::Before), 4); // at insert, track before
        assert_eq!(cs.map_pos(4, Assoc::After), 6); // at insert, track after
        assert_eq!(cs.map_pos(5, Assoc::Before), 7); // after insert region

        // maps deletes
        let cs = ChangeSet {
            changes: vec![Retain(4), Delete(4), Retain(4)],
            len: 12,
        };
        assert_eq!(cs.map_pos(0, Assoc::Before), 0); // at start
        assert_eq!(cs.map_pos(4, Assoc::Before), 4); // before a delete
        assert_eq!(cs.map_pos(5, Assoc::Before), 4); // inside a delete
        assert_eq!(cs.map_pos(5, Assoc::After), 4); // inside a delete

        // TODO: delete tracking

        // stays inbetween replacements
        let cs = ChangeSet {
            changes: vec![
                Delete(2),
                Insert("ab".into()),
                Delete(2),
                Insert("cd".into()),
            ],
            len: 4,
        };
        assert_eq!(cs.map_pos(2, Assoc::Before), 2);
        assert_eq!(cs.map_pos(2, Assoc::After), 2);
    }
}