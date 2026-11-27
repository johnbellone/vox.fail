//! The in-world editor: the edit mode a player turns on in a world they may change. It is
//! made of the [`bridge`] (the messages between the page and the client), and, in later
//! steps, the edit camera, picking, ghosts, gizmos and undo. Nothing here runs in Bevy yet.

pub mod bridge;
