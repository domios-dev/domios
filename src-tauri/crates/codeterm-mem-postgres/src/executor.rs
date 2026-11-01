use postgres::{
    types::ToSql, Config, Error, IsolationLevel, NoTls, Row, SimpleQueryMessage, Statement,
};
use rayon::{ThreadPool, ThreadPoolBuilder};
use std::sync::Arc;

struct Owned<T: Send> {
    value: Option<T>,
    executor: Arc<ThreadPool>,
}

impl<T: Send> Owned<T> {
    fn with<'a, R: Send>(&'a mut self, f: impl FnOnce(&'a mut T) -> R + Send) -> R {
        let value = self
            .value
            .as_mut()
            .expect("executor value exists until drop");
        self.executor.install(|| f(value))
    }

    fn take<R: Send>(mut self, f: impl FnOnce(T) -> R + Send) -> R {
        let value = self.value.take().expect("executor value exists until drop");
        self.executor.install(|| f(value))
    }
}

impl<T: Send> Drop for Owned<T> {
    fn drop(&mut self) {
        if let Some(value) = self.value.take() {
            self.executor.install(|| drop(value));
        }
    }
}

pub(crate) struct Client(Owned<postgres::Client>);
pub(crate) struct Transaction<'a>(Owned<postgres::Transaction<'a>>);
pub(crate) struct TransactionBuilder<'a>(Owned<postgres::TransactionBuilder<'a>>);

impl Client {
    pub(crate) fn connect(config: &Config) -> crate::StoreResult<Self> {
        Self::connect_with(|| config.connect(NoTls))
    }

    fn connect_with(
        connect: impl FnOnce() -> Result<postgres::Client, Error> + Send,
    ) -> crate::StoreResult<Self> {
        let executor = Arc::new(
            ThreadPoolBuilder::new()
                .num_threads(1)
                .thread_name(|_| "mem-postgres".into())
                .build()
                .map_err(|_| "Postgres memory connection failed")?,
        );
        let value = executor.install(connect).map_err(crate::db_error)?;
        Ok(Self(Owned {
            value: Some(value),
            executor,
        }))
    }

    pub(crate) fn build_transaction(&mut self) -> TransactionBuilder<'_> {
        let executor = self.0.executor.clone();
        let value = self.0.with(|client| client.build_transaction());
        TransactionBuilder(Owned {
            value: Some(value),
            executor,
        })
    }

    pub(crate) fn transaction(&mut self) -> Result<Transaction<'_>, Error> {
        self.build_transaction().start()
    }
}

impl TransactionBuilder<'_> {
    pub(crate) fn read_only(self, read_only: bool) -> Self {
        let executor = self.0.executor.clone();
        let value = self.0.take(|builder| builder.read_only(read_only));
        Self(Owned {
            value: Some(value),
            executor,
        })
    }

    pub(crate) fn isolation_level(self, isolation: IsolationLevel) -> Self {
        let executor = self.0.executor.clone();
        let value = self.0.take(|builder| builder.isolation_level(isolation));
        Self(Owned {
            value: Some(value),
            executor,
        })
    }
}

impl<'a> TransactionBuilder<'a> {
    pub(crate) fn start(self) -> Result<Transaction<'a>, Error> {
        let executor = self.0.executor.clone();
        let value = self.0.take(|builder| builder.start())?;
        Ok(Transaction(Owned {
            value: Some(value),
            executor,
        }))
    }
}

impl Transaction<'_> {
    pub(crate) fn commit(self) -> Result<(), Error> {
        self.0.take(|tx| tx.commit())
    }
}

pub(crate) trait GenericClient {
    fn query(&mut self, query: &str, params: &[&(dyn ToSql + Sync)]) -> Result<Vec<Row>, Error>;
    fn query_one(&mut self, query: &str, params: &[&(dyn ToSql + Sync)]) -> Result<Row, Error>;
    fn query_opt(
        &mut self,
        query: &str,
        params: &[&(dyn ToSql + Sync)],
    ) -> Result<Option<Row>, Error>;
}

macro_rules! query_methods {
    ($name:ident, $result:ty) => {
        pub(crate) fn $name(
            &mut self,
            query: &str,
            params: &[&(dyn ToSql + Sync)],
        ) -> Result<$result, Error> {
            self.0.with(|client| client.$name(query, params))
        }
    };
}

macro_rules! generic_client {
    () => {
        fn query(
            &mut self,
            query: &str,
            params: &[&(dyn ToSql + Sync)],
        ) -> Result<Vec<Row>, Error> {
            self.query(query, params)
        }
        fn query_one(&mut self, query: &str, params: &[&(dyn ToSql + Sync)]) -> Result<Row, Error> {
            self.query_one(query, params)
        }
        fn query_opt(
            &mut self,
            query: &str,
            params: &[&(dyn ToSql + Sync)],
        ) -> Result<Option<Row>, Error> {
            self.query_opt(query, params)
        }
    };
}

impl Client {
    query_methods!(query, Vec<Row>);
    query_methods!(query_one, Row);
    query_methods!(query_opt, Option<Row>);

    pub(crate) fn simple_query(&mut self, query: &str) -> Result<Vec<SimpleQueryMessage>, Error> {
        self.0.with(|client| client.simple_query(query))
    }

    pub(crate) fn batch_execute(&mut self, query: &str) -> Result<(), Error> {
        self.0.with(|client| client.batch_execute(query))
    }
}

impl Transaction<'_> {
    query_methods!(query, Vec<Row>);
    query_methods!(query_one, Row);
    query_methods!(query_opt, Option<Row>);
    query_methods!(execute, u64);

    pub(crate) fn prepare(&mut self, query: &str) -> Result<Statement, Error> {
        self.0.with(|client| client.prepare(query))
    }

    pub(crate) fn batch_execute(&mut self, query: &str) -> Result<(), Error> {
        self.0.with(|client| client.batch_execute(query))
    }
}

impl GenericClient for Client {
    generic_client!();
}

impl GenericClient for Transaction<'_> {
    generic_client!();
}

#[derive(Debug)]
pub(crate) struct Manager(r2d2_postgres::PostgresConnectionManager<NoTls>);

impl Manager {
    pub(crate) fn new(config: Config) -> Self {
        Self(r2d2_postgres::PostgresConnectionManager::new(config, NoTls))
    }
}

impl r2d2::ManageConnection for Manager {
    type Connection = Client;
    type Error = std::io::Error;

    fn connect(&self) -> Result<Client, Self::Error> {
        Client::connect_with(|| self.0.connect()).map_err(std::io::Error::other)
    }

    fn is_valid(&self, client: &mut Client) -> Result<(), Self::Error> {
        client
            .0
            .with(|client| self.0.is_valid(client))
            .map_err(std::io::Error::other)
    }

    fn has_broken(&self, client: &mut Client) -> bool {
        client.0.with(|client| self.0.has_broken(client))
    }
}

#[cfg(test)]
mod tests;
