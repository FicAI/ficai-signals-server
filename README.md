# Fic.AI signals server

The backend component that provides the API for accessing and modifying a user's contribution to the Fic.AI database (postgres) in the form of "signals".

Each signal indicates approval or disapproval of a tag (identified by name) being applied to a story (identified by its canonical URL).

## Running the server

The server expects the following environment variables to be set:
* `FICAI_LISTEN` is the socket address on which the API will be available. Example: `127.0.0.1:8080`
* `FICAI_DB_HOST` is the host on which the DB server can be accessed. Example: `localhost`
* `FICAI_DB_PORT` is the port on which the DB server is listening for connections. Example: `5432`
* `FICAI_DB_USERNAME` is the user name for DB access
* `FICAI_DB_PASSWORD` is the password for the abovementioned user
* `FICAI_DB_DATABASE` is the name of the database in which the server's tables must be present
* `FICAI_PWD_PEPPER` is the pepper value for password hashes. Must be specified as unpadded Base64 with standard alphabet as defined in [RFC-4648]. Recommended minimum length (before Base64 encoding) is 32 bytes, see [Kitten]. You can generate a suitable value by running `openssl rand -base64 32` and stripping any `=` characters from the end of its output. Read more about peppering passwords at [OWASP-PSCS].
* `FICAI_DOMAIN` is the domain (no URL schema or port!) on which the service will be accessible. Used for the session ID cookie.
* `FICAI_BETA_KEY` is the extra key a user must give on registration during the inital beta period.

[OWASP-PSCS]: https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html#peppering
[Kitten]: https://www.ietf.org/archive/id/draft-ietf-kitten-password-storage-04.html#section-4.2
[RFC-4648]: https://datatracker.ietf.org/doc/html/rfc4648

# Running with docker-compose
1. Create `.env` file based on `test.env.template` without `FICAI_DB_HOST` and `FICAI_DB_PORT` to set environment variables
2. Install Docker (or Docker Desktop for Mac/Windows)
3. Run `docker-compose up -d --build`. SQL migrations in `schema.sql` will run automatically on first launch
