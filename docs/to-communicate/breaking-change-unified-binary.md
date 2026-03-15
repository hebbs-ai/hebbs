# Breaking Change: Unified Binary

## What changed

`hebbs-cli` and `hebbs-vault` are replaced by a single `hebbs` binary. Both old binary names will stop working after upgrading.

| Old command | New command |
|-------------|-------------|
| `hebbs-cli remember "..."` | `hebbs remember "..."` |
| `hebbs-cli recall "..."` | `hebbs recall "..."` |
| `hebbs-cli prime ...` | `hebbs prime ...` |
| `hebbs-cli forget ...` | `hebbs forget ...` |
| `hebbs-cli reflect ...` | `hebbs reflect ...` |
| `hebbs-vault init .` | `hebbs init .` |
| `hebbs-vault index .` | `hebbs index .` |
| `hebbs-vault watch .` | `hebbs watch .` |
| `hebbs-vault rebuild .` | `hebbs rebuild .` |
| `hebbs-vault status` | `hebbs status` |

All flags, output formats, and behavior are identical. Only the binary name changes.

## Who is affected

- Scripts or automation that call `hebbs-cli` or `hebbs-vault` by name
- Agent skills or system prompts that reference `hebbs-cli`
- Shell aliases or PATH entries pointing to old binary names
- CI/CD pipelines that invoke either old binary

## Backward compatibility via Homebrew symlinks

The Homebrew formula should create symlinks so both old names continue to work:

```ruby
# In Formula/hebbs.rb
def install
  bin.install "hebbs"
  bin.install_symlink "hebbs" => "hebbs-cli"
  bin.install_symlink "hebbs" => "hebbs-vault"
end
```

Users who install via Homebrew get backward compatibility for free. Both `hebbs-cli recall "..."` and `hebbs recall "..."` work identically.

Users who install via the install script or build from source will need to update their commands or create symlinks manually:

```sh
ln -s $(which hebbs) /usr/local/bin/hebbs-cli
ln -s $(which hebbs) /usr/local/bin/hebbs-vault
```

## Migration checklist

- [ ] Update SKILL.md: replace all `hebbs-cli` references with `hebbs`
- [ ] Update agent system prompts and policies that reference `hebbs-cli`
- [ ] Update CI scripts that call `hebbs-vault` or `hebbs-cli`
- [ ] Update Homebrew formula to produce `hebbs` binary with symlinks
- [ ] Update install script to install `hebbs` (not `hebbs-server` or `hebbs-vault`)
- [ ] Add deprecation note in release notes

## Timeline

- v0.2.0: `hebbs` is the only binary produced. Homebrew symlinks provide backward compatibility.
- v0.3.0 (tentative): remove symlinks. `hebbs-cli` and `hebbs-vault` stop working entirely.
