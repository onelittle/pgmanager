![GitHub Actions Workflow Status](https://img.shields.io/github/actions/workflow/status/onelittle/pgmanager/ci.yml)
![Crates.io Version](https://img.shields.io/crates/v/pgmanager)
![Crates.io License](https://img.shields.io/crates/l/pgmanager)

# pgmanager

`pgmanager` is a utility for managing PostgreSQL databases in parallelized test environments, where tests are sharded across multiple processes. It provides mutually exclusive database assignment via a lightweight server exposed over a UNIX domain socket.

The primary goal is to give each test process exclusive access to a database instance, avoiding cross-test interference while remaining simple and fast.

## installation

### with cargo

```shell
cargo install pgmanager
```

### client-side

```toml
[dev-dependencies]
pgmanager = "0.3.1"
```

### nix flake

```nix
{
  inputs = {
    pgmanager.url = "github:onelittle/pgmanager";
  };

  outputs = {
    pgmanager,
    ...
  }: {
    devShell = pkgs.mkShell {
      packages = [
        pgmanager.packages.${system}.default
      ];
    };
  }
}
```

## how it works

Conceptually, `pgmanager` is a tiny in-memory database (name) pool exposed over a UNIX socket.

* The server maintains a pool of database names.
* Each client connection is assigned one database exclusively.
* The assignment is held for the lifetime of the connection.
* When the connection closes, the database is released back into the pool.
* Databases are assigned using a round-robin strategy.

Important constraints:

* Databases must be created ahead of time.
* `pgmanager` does not reset database state.
* Test code is responsible for isolation via transactions, rollbacks, or other mechanisms.

Database initialization can be made easier by using `pgmanager wrap-each` (see below).

## usage

### pgmanager serve

Runs the `pgmanager` server independently. Clients connect via a UNIX socket and request a database.

Configuration is driven entirely by environment variables:

* `PGM_SOCKET` – path to the UNIX socket
* `PGM_DATABASE_PREFIX` – database name prefix
* `PGM_DATABASE_COUNT` – number of databases in the pool

```shell
# Serve a pool of 16 postgres databases
export PGM_DATABASE_PREFIX="myapp_test"
export PGM_DATABASE_COUNT="16"
export PGM_SOCKET="/tmp/pgm.sock"
pgmanager serve
```

```rust
#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test() {
        let db_name = pgmanager::get_database().await;
        eprintln!("A database is available at {}", db_name);
    }
}
```

```shell
export PGM_SOCKET="/tmp/pgm.sock"
cargo test
```

### pgmanager wrap

Runs the server and client as one command. If specified, the `PGM_SOCKET` environment is used and passed to the subcommand. If no value is provided it will default to `tmp/pgmanager.sock`.

Rust integration is the same as above.

```shell
# Run tests with a pool of 16 postgres databases
export PGM_DATABASE_PREFIX="myapp_test"
export PGM_DATABASE_COUNT="16"
pgmanager wrap -- cargo test
```

### pgmanager wrap-each

Used to initialize and clean the test environment. Passes `PGDATABASE` to the subcommand. See `pgmanager wrap-each --help` for details.

```shell
# Create and drop 16 postgres databases
export PGM_DATABASE_PREFIX="myapp_test"
export PGM_DATABASE_COUNT="16"
pgmanager wrap-each -- createdb
pgmanager wrap-each --xargs -- dropdb
```

## why

Transactions alone are sometimes insufficient for test isolation in parallel environments:

* Global state (extensions, sequences, advisory locks)
* DDL operations
* Connection-level settings
* Tests that intentionally commit

`pgmanager` enables stricter isolation (than just transactions) while still allowing parallel execution.
