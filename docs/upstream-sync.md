# Upstream mirror credentials

The scheduled sync copies DioxusLabs/blitz's `main` commit unchanged to
`upstream-main`. Upstream commits can include `.github/workflows` changes,
which GitHub refuses when pushed with the built-in `GITHUB_TOKEN`.

The workflow therefore uses a dedicated SSH deploy key with write access to
this repository. Store the private key in the repository Actions secret
`UPSTREAM_SYNC_SSH_KEY` and register its public key under **Settings > Deploy
keys** with **Allow write access** enabled. Use a separate key for each mirror
repository; do not reuse a personal SSH key or access token. The built-in token
remains read-only.

The workflow fails explicitly if the secret is missing, updates only
`upstream-main`, and uses `--force-with-lease` against its observed head. It
never merges upstream into `main`. The deploy key itself permits repository
writes, so protect access to the secret and workflow changes accordingly.

Deploy keys have no automatic expiration. To rotate one, register a replacement
public key, update the Actions secret, run **Sync upstream mirror** manually,
and verify that `upstream-main` matches the upstream commit before removing the
old deploy key. Remove both the deploy key and secret to revoke access.

A deploy-key push may trigger workflows defined in the upstream commit. Review
their branch filters when changing the mirrored source or Actions policy.

See [GitHub's deploy key documentation](https://docs.github.com/en/authentication/connecting-to-github-with-ssh/managing-deploy-keys)
for repository scoping and key management.
