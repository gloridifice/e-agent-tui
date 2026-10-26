//! Frontend conversation identities and two-slot ownership, independent of providers.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(u64);

#[derive(Debug)]
pub struct SessionEnvelope<T> {
    pub session: SessionId,
    pub payload: T,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideSessionKind {
    Clone,
    New,
    Fork,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideSessionSeed {
    /// Opaque backend attachment identity; absent for a not-yet-persisted branch.
    pub session_id: Option<String>,
    pub workspace: String,
    pub model: Option<crate::agent::ModelSelection>,
    pub editor_text: String,
    pub parent_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidePreparationResult {
    Ready(SideSessionSeed),
    Cancelled,
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideSessionCommand {
    Toggle,
    Create(SideSessionKind),
    Close,
    Swap,
}

impl SideSessionCommand {
    pub fn parse(arguments: &str) -> Result<Self, &'static str> {
        let mut words = arguments.split_whitespace();
        let command = match words.next() {
            None => Self::Toggle,
            Some("clone") => Self::Create(SideSessionKind::Clone),
            Some("new") => Self::Create(SideSessionKind::New),
            Some("fork") => Self::Create(SideSessionKind::Fork),
            Some("close") => Self::Close,
            Some("swap") => Self::Swap,
            Some(_) => return Err("Usage: /side [clone|new|fork|close|swap]"),
        };
        if words.next().is_some() {
            return Err("Usage: /side [clone|new|fork|close|swap]");
        }
        Ok(command)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SideCreation {
    session: SessionId,
    source: SessionId,
    kind: SideSessionKind,
}

impl SideCreation {
    pub fn session(self) -> SessionId {
        self.session
    }

    pub fn source(self) -> SessionId {
        self.source
    }

    pub fn kind(self) -> SideSessionKind {
        self.kind
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionWorkspaceError {
    SideAlreadyOpen,
    CreationPending,
    NoSideSession,
    UnknownSession,
    IdentityExhausted,
}

impl fmt::Display for SessionWorkspaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::SideAlreadyOpen => "Close the existing side session before creating another",
            Self::CreationPending => "A side session is already being created",
            Self::NoSideSession => "No side session is open",
            Self::UnknownSession => "The conversation is no longer open",
            Self::IdentityExhausted => "No more conversation identities are available",
        })
    }
}

impl std::error::Error for SessionWorkspaceError {}

#[derive(Debug)]
pub struct SessionSlot<T> {
    id: SessionId,
    pub value: T,
}

impl<T> SessionSlot<T> {
    pub fn id(&self) -> SessionId {
        self.id
    }
}

pub struct SessionWorkspace<T> {
    main: SessionSlot<T>,
    side: Option<SessionSlot<T>>,
    focused: SessionId,
    pending: Option<SideCreation>,
    last_id: u64,
}

impl<T> SessionWorkspace<T> {
    pub fn new(main: T) -> Self {
        let id = SessionId(1);
        Self {
            main: SessionSlot { id, value: main },
            side: None,
            focused: id,
            pending: None,
            last_id: id.0,
        }
    }

    pub fn main(&self) -> &SessionSlot<T> {
        &self.main
    }

    pub fn side(&self) -> Option<&SessionSlot<T>> {
        self.side.as_ref()
    }

    pub fn focused(&self) -> SessionId {
        self.focused
    }

    pub fn pending(&self) -> Option<SideCreation> {
        self.pending
    }

    pub fn slots(&self) -> impl Iterator<Item = &SessionSlot<T>> {
        std::iter::once(&self.main).chain(self.side.iter())
    }

    pub fn get(&self, id: SessionId) -> Option<&T> {
        self.slots()
            .find(|slot| slot.id == id)
            .map(|slot| &slot.value)
    }

    pub fn get_mut(&mut self, id: SessionId) -> Option<&mut T> {
        if self.main.id == id {
            Some(&mut self.main.value)
        } else {
            self.side
                .as_mut()
                .filter(|slot| slot.id == id)
                .map(|slot| &mut slot.value)
        }
    }

    pub fn focus(&mut self, id: SessionId) -> Result<(), SessionWorkspaceError> {
        if self.get(id).is_none() {
            return Err(SessionWorkspaceError::UnknownSession);
        }
        self.focused = id;
        Ok(())
    }

    pub fn focus_other(&mut self) -> Result<SessionId, SessionWorkspaceError> {
        let side = self
            .side
            .as_ref()
            .ok_or(SessionWorkspaceError::NoSideSession)?;
        self.focused = if self.focused == self.main.id {
            side.id
        } else {
            self.main.id
        };
        Ok(self.focused)
    }

    pub fn begin_side(
        &mut self,
        kind: SideSessionKind,
    ) -> Result<SideCreation, SessionWorkspaceError> {
        if self.side.is_some() {
            return Err(SessionWorkspaceError::SideAlreadyOpen);
        }
        if self.pending.is_some() {
            return Err(SessionWorkspaceError::CreationPending);
        }
        self.last_id = self
            .last_id
            .checked_add(1)
            .ok_or(SessionWorkspaceError::IdentityExhausted)?;
        let creation = SideCreation {
            session: SessionId(self.last_id),
            source: self.focused,
            kind,
        };
        self.pending = Some(creation);
        Ok(creation)
    }

    /// A rejected candidate is returned so its owner can shut down external resources.
    pub fn finish_side(&mut self, creation: SideCreation, value: T) -> Result<SessionId, T> {
        if self.pending != Some(creation)
            || self.side.is_some()
            || self.get(creation.source).is_none()
        {
            return Err(value);
        }
        self.pending = None;
        self.side = Some(SessionSlot {
            id: creation.session,
            value,
        });
        self.focused = creation.session;
        Ok(creation.session)
    }

    pub fn cancel_side(&mut self, creation: SideCreation) -> bool {
        if self.pending != Some(creation) {
            return false;
        }
        self.pending = None;
        true
    }

    pub fn swap(&mut self) -> Result<(), SessionWorkspaceError> {
        let side = self
            .side
            .as_mut()
            .ok_or(SessionWorkspaceError::NoSideSession)?;
        std::mem::swap(&mut self.main, side);
        Ok(())
    }

    /// The caller confirms busy closure first, then shuts down the returned owner.
    pub fn close(&mut self, id: SessionId) -> Result<SessionSlot<T>, SessionWorkspaceError> {
        if self.get(id).is_none() {
            return Err(SessionWorkspaceError::UnknownSession);
        }
        let side = self
            .side
            .take()
            .ok_or(SessionWorkspaceError::NoSideSession)?;
        let closed = if self.main.id == id {
            std::mem::replace(&mut self.main, side)
        } else {
            side
        };
        self.focused = self.main.id;
        Ok(closed)
    }
}
