import assert from 'node:assert/strict'
import { mkdirSync, mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { localRegistrySources } from './prepare-crate-release.mjs'

test('release cache selection is limited to the package version in path-backed registries', () => {
  const cargoHome = mkdtempSync(join(tmpdir(), 'release-cache-'))
  try {
    const sourceRoot = join(cargoHome, 'registry', 'src')
    const expected = [
      join(sourceRoot, '-b511af9481ee4495', 'e-tui-0.0.1'),
      join(sourceRoot, '-0123456789abcdef', 'e-tui-0.0.1'),
    ]
    const preserved = [
      join(sourceRoot, 'index.crates.io-1949cf8c6b5b557f', 'e-tui-0.0.1'),
      join(sourceRoot, 'custom.example-0123456789abcdef', 'e-tui-0.0.1'),
      join(sourceRoot, '-b511af9481ee4495', 'e-tui-0.0.2'),
      join(sourceRoot, '-b511af9481ee4495', 'e-pi-0.0.1'),
    ]
    for (const path of [...expected, ...preserved]) mkdirSync(path, { recursive: true })

    assert.deepEqual(localRegistrySources(cargoHome, 'e-tui', '0.0.1').sort(), expected.sort())
    assert.deepEqual(localRegistrySources(cargoHome, 'e-dsh', '0.0.1'), [])
  } finally {
    rmSync(cargoHome, { recursive: true, force: true })
  }
})

test('a fresh Cargo home needs no source cache cleanup', () => {
  const cargoHome = mkdtempSync(join(tmpdir(), 'release-cache-'))
  try {
    assert.deepEqual(localRegistrySources(cargoHome, 'e-tui', '0.0.1'), [])
  } finally {
    rmSync(cargoHome, { recursive: true, force: true })
  }
})
