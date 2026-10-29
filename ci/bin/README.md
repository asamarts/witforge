# Pinned aplan binary

`aplan.gz` is the self-hosted CI's aplan executable. It is a static x86_64 Linux build of the
staged `0.5.0-dev` tool used by this pilot; the matching `aplan.sha256` is checked by
`ci/install-aplan.sh` before decompression. The binary is committed because no published aplan
0.5 release exists yet and the private self-hosted runner must not depend on a developer's
`~/.local/bin`.

Replace `aplan.gz` and `aplan.sha256` together when a published release with the same plan
surface is available. CI fails closed on a checksum or version mismatch.
