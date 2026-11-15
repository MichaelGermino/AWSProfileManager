Test-only certificates for the parity suite's fake servers (tests/fakes).

- `ca.pem` / `ca.key`: a throwaway CA. The app trusts it only in test mode, via APM_TEST_CA_FILE.
- `server.pem` / `server.key`: issued by that CA for 127.0.0.1 and localhost, valid 20 years.

These keys protect nothing. Never install `ca.pem` into an OS trust store outside a disposable
test machine. Regenerate with the commands in the commit that added this folder.
