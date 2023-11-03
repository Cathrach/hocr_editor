use crate::InternalID;
use std::collections::HashMap;

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
    value: D,
    parent: Option<InternalID>,
    children: Vec<InternalID>,
    id: InternalID,
}

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
    pub fn push_child(&mut self, id: &InternalID, child: D) -> Option<InternalID> {
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
            return Some(new_id);
        }
        None
    }

    // add a sibling to a node
    pub fn add_sibling(
        &mut self,
        id: &InternalID,
        sibling: D,
        pos: Position,
    ) -> Option<InternalID> {
        // if id exists, find node's parent
        // if node's parent doesn't exist, add a root
        // if node's parent exists
        // insert sibling into the hash map
        // insert sibling's ID into the parent's child vector before id
        if let Some(node) = self.nodes.get(id) {
            if let Some(par_id) = node.parent {
                let new_id = self.curr_id;
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
                let par_child_index = self
                    .nodes
                    .get(&par_id)
                    .unwrap()
                    .children
                    .binary_search(id)
                    .unwrap();
                let insert_index = par_child_index
                    + match pos {
                        Position::After => 1,
                        Position::Before => 0,
                    };
                self.nodes
                    .get_mut(&par_id)
                    .unwrap()
                    .children
                    .insert(insert_index, *id);
                return Some(new_id);
            } else {
                return Some(self.add_root(sibling));
            }
        } else {
            None
        }
    }

    // get a (ref to) node value by ID -- wrapper around hash map function
    pub fn get_node(&self, id: &InternalID) -> Option<&D> {
        match self.nodes.get(id) {
            Some(node) => Some(&node.value),
            None => None,
        }
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
        let par = self.nodes.get_mut(par_id).unwrap();
        let index = par.children.binary_search(child_id).unwrap();
        par.children.remove(index);
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
                    let index = self.roots.binary_search(id).unwrap();
                    self.roots.remove(index);
                }
                Some(par_id) => self.delete_child_from_parent(&par_id, id),
            }
        }
    }
}
