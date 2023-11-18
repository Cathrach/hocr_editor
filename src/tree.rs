use crate::InternalID;
use std::collections::HashMap;
use std::slice::Iter;

// the "tree" is a dictionary of IDs to nodes
#[derive(Default, Debug)]
pub struct Tree<D> {
    nodes: HashMap<InternalID, Node<D>>,
    roots: Vec<InternalID>,
    curr_id: InternalID,
}

#[derive(Debug)]
// a node has a value, a parent (an ID), and children (a vector of IDs)
// yes, removing and inserting are O(n), but whatever, I need order to be preserved
pub struct Node<D> {
    pub value: D,
    pub parent: Option<InternalID>,
    pub children: Vec<InternalID>,
    pub id: InternalID,
}

#[derive(Debug)]
pub enum Position {
    Before,
    After,
}

impl<D> Tree<D> {
    // return an empty tree
    pub fn new() -> Self {
        Tree {
            nodes: HashMap::new(),
            roots: Vec::new(),
            curr_id: 0,
        }
    }

    // add a node as a root
    pub fn add_root(&mut self, root: D) -> InternalID {
        let id = self.curr_id;
        self.nodes.insert(
            id,
            Node {
                value: root,
                parent: None,
                children: Vec::new(),
                id: id,
            },
        );
        self.roots.push(id);
        self.curr_id += 1;
        id
    }

    // add a child to the end of id's children
    pub fn push_child(&mut self, id: &InternalID, child: D) -> Result<InternalID, String> {
        if let Some(parent) = self.nodes.get_mut(id) {
            let new_id = self.curr_id;
            parent.children.push(new_id);
            self.nodes.insert(
                new_id,
                Node {
                    value: child,
                    parent: Some(*id),
                    children: Vec::new(),
                    id: new_id,
                },
            );
            self.curr_id += 1;
            Ok(new_id)
        } else {
            Err(format!("push_child: node {} doesn't exist!", id))
        }
    }

    // add a sibling to a node
    // return error if id doesn't exist in the tree
    pub fn add_sibling(
        &mut self,
        id: &InternalID,
        sibling: D,
        pos: &Position,
    ) -> Result<InternalID, String> {
        // if id exists, find node's parent
        // if node's parent doesn't exist, add a root
        // if node's parent exists
        // insert sibling into the hash map
        // insert sibling's ID into the parent's child vector before id
        if let Some(node) = self.nodes.get(id) {
            return if let Some(par_id) = node.parent {
                let new_id = self.curr_id;
                println!("add_sibling: sib has id {}", new_id);
                println!("add_sibling: I have id {}", id);
                self.nodes.insert(
                    new_id,
                    Node {
                        value: sibling,
                        parent: Some(par_id),
                        children: Vec::new(),
                        id: new_id,
                    },
                );
                self.curr_id += 1;
                // this error is fatal because it means our internal representation of the tree is wrong
                let par_child_index = self.children(&par_id).position(|&x| x == *id).expect(
                    format!("Couldn't find {} among parent {}'s children", id, par_id).as_str(),
                );
                let insert_index = par_child_index
                    + match pos {
                        Position::After => 1,
                        Position::Before => 0,
                    };
                self.nodes
                    .get_mut(&par_id)
                    .expect(format!("parent {} of {} doesn't exist", par_id, id).as_str())
                    .children
                    .insert(insert_index, new_id);
                Ok(new_id)
            } else {
                Ok(self.add_root(sibling))
            };
        } else {
            Err(format!("add_sibling: node {} doesn't exist!", id))
        }
    }

    // get a (ref to) node value by ID -- wrapper around hash map function
    pub fn get_node(&self, id: &InternalID) -> Option<&D> {
        self.nodes.get(id).map(|node| &node.value)
    }

    // TODO: I decided that if the node doesn't exist, the children should just be an empty iterator
    pub fn children(&self, id: &InternalID) -> Iter<'_, InternalID> {
        self.nodes
            .get(id)
            .map(|node| node.children.iter())
            .unwrap_or_default()
    }

    pub fn parent(&self, id: &InternalID) -> Option<InternalID> {
        self.nodes.get(id).map(|node| node.parent).unwrap_or(None)
    }

    // if the node doesn't exist, it has no siblings, but we can run into errors in the tree
    fn siblings(&self, id: &InternalID) -> Option<&Vec<InternalID>> {
        self.nodes.get(id).map(|node| {
            if let Some(par_id) = node.parent {
                let par_node = self
                    .nodes
                    .get(&par_id)
                    .expect(format!("node {}'s parent {} doesn't exist", id, par_id).as_str());
                &par_node.children
            } else {
                &self.roots
            }
        })
    }

    // empty iterator if node doesn't exist
    // actually fails if the tree is wrong
    pub fn prev_siblings(&self, id: &InternalID) -> Iter<'_, InternalID> {
        self.siblings(id)
            .map(|siblings| {
                let my_index = siblings
                    .iter()
                    .position(|&x| x == *id)
                    .expect(format!("couldn't find {} among siblings {:?}", id, siblings).as_str());
                siblings[..my_index].iter()
            })
            .unwrap_or_default()
    }

    // TODO: return the merged sibling
    pub fn merge_sibling(&mut self, id: &InternalID, pos: &Position) {
        let sib_id = match pos {
            Position::After => self.next_sibling(id),
            Position::Before => self.prev_sibling(id),
        };
        println!("Merging {} with {:?}", id, sib_id);
        if sib_id.is_none() {
            return;
        }
        let sibling_id = sib_id.expect("failed te return even though sibling was None");
        let mut sib_children: Vec<InternalID> = self.children(&sibling_id).cloned().collect();
        // reparent each sib_child
        for child_id in &sib_children {
            if let Some(node) = self.nodes.get_mut(child_id) {
                println!("merge sibling: reparented {} to {}", child_id, id);
                node.parent = Some(*id);
            }
        }
        // reparent id + pos' children after id's children
        if let Some(node) = self.nodes.get_mut(id) {
            match pos {
                Position::After => node.children.extend(sib_children.iter()),
                Position::Before => {
                    sib_children.extend(node.children.clone());
                    node.children = sib_children;
                }
            }
            println!("merge_sibling: new children {:?}", node.children);
        }

        self.nodes
            .get_mut(&sibling_id)
            .expect(format!("sibling {} of {} didn't exist", sibling_id, id).as_str())
            .children = Vec::new();
        self.delete_node(&sibling_id);
    }

    pub fn next_sibling(&self, id: &InternalID) -> Option<InternalID> {
        self.next_siblings(id).next().copied()
    }

    pub fn prev_sibling(&self, id: &InternalID) -> Option<InternalID> {
        if let Some(node) = self.nodes.get(id) {
            let siblings = match node.parent {
                Some(par_id) => {
                    &self
                        .nodes
                        .get(&par_id)
                        .expect(format!("parent {} of {} doesn't exist", par_id, id).as_str())
                        .children
                }
                None => &self.roots,
            };
            let my_index = siblings
                .iter()
                .position(|&x| x == *id)
                .expect(format!("couldn't find {} among siblings {:?}", id, siblings).as_str());
            if my_index > 0 {
                Some(siblings[my_index - 1])
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn next_siblings(&self, id: &InternalID) -> Iter<'_, InternalID> {
        self.siblings(id)
            .map(|siblings| {
                let my_index =
                    siblings.iter().position(|&x| x == *id).expect(
                        format!("couldn't find {} among siblings {:?}", id, siblings).as_str(),
                    ) + 1;
                siblings[my_index..].iter()
            })
            .unwrap_or_default()
    }

    pub fn has_children(&self, id: &InternalID) -> bool {
        match self.nodes.get(id) {
            Some(node) => node.children.len() > 0,
            None => false,
        }
    }

    pub fn roots(&self) -> Iter<'_, InternalID> {
        self.roots.iter()
    }

    // mutable ref to node val by ID -- used when we need to modify bbox or text
    pub fn get_mut_node(&mut self, id: &InternalID) -> Option<&mut D> {
        match self.nodes.get_mut(id) {
            Some(node) => Some(&mut node.value),
            None => None,
        }
    }

    // this is only a helper! never call it outside!
    fn delete_child_from_parent(&mut self, par_id: &InternalID, child_id: &InternalID) {
        let index = self.children(par_id).position(|&x| x == *child_id); // par.children.binary_search(child_id).unwrap();
        let par = self
            .nodes
            .get_mut(par_id)
            .expect(format!("child {}'s parent {} doesn't exist", child_id, par_id).as_str());
        if let Some(id) = index {
            par.children.remove(id);
        }
    }

    // helper for delete_node
    // this doesn't disconnect a node from its parent, it just recursively removes a node and its children
    // any node passed in here will just get removed from the hashmap
    // it returns whether the node actually existed and the parent ID for use in delete_node
    fn delete_rec_node(&mut self, id: &InternalID) -> (bool, Option<InternalID>) {
        let removed = self.nodes.remove(id);
        if let Some(node) = removed {
            for child in node.children {
                self.delete_rec_node(&child);
            }
            return (true, node.parent);
        }
        return (false, None);
    }

    // delete a node from the tree. This ALSO DELETES ITS CHILDREN!
    pub fn delete_node(&mut self, id: &InternalID) {
        // remove the node and its children from hashmap
        let (existed, parent_id) = self.delete_rec_node(id);
        if existed {
            match parent_id {
                // node is a root
                None => {
                    let index = self.roots.iter().position(|&x| x == *id); // self.roots.binary_search(id).unwrap();
                    if let Some(ind) = index {
                        self.roots.remove(ind);
                    }
                }
                Some(par_id) => self.delete_child_from_parent(&par_id, id),
            }
        }
    }
}
