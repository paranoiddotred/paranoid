# Contributing

Thank you for looking. Bug reports, test cases and fixes are all welcome.

## Before you open a pull request

1. Build with `bash scripts/build.sh` and run everything with `bash scripts/test.sh`. Both must pass. The
   test script starts its own local validator on port 18929 (set `PARANOID_RPC_PORT` to move it) and stops
   it when it ends.
2. Keep one change per pull request, and add a test that fails without it.
3. Write commit messages as [conventional commits](https://www.conventionalcommits.org), for example
   `fix: refuse a top up past the membership limit`.

The program is live on mainnet, so a change to `programs/paranoid` only reaches users through an upgrade.
A change to `circuits/request.circom` also needs a new trusted setup (see CEREMONY.md).

## Security problems

Please do not open a public issue. Use **Report a vulnerability** on the Security tab, as SECURITY.md
explains.
