pub struct User {
    pub id: u64,
    pub name: String,
}

pub enum AuthError {
    NotFound,
    BadPassword,
}

pub fn authenticate_user(name: &str, pw: &str) -> Result<User, AuthError> {
    let _ = (name, pw);
    Err(AuthError::NotFound)
}

pub fn login(user: &User) -> bool {
    user.id != 0
}

impl User {
    pub fn new(id: u64, name: String) -> Self {
        User { id, name }
    }
}
