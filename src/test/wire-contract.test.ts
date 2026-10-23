import { describe, expect, it } from 'vitest';
import fixtureJson from '../../tests/fixtures/contract-v1.json';
import { parseByteString } from '../telemetry/format';
import {
  LABEL_MAX_BYTES,
  MESSAGE_MAX_BYTES,
  stripControl,
  truncateUtf8,
  utf8Length,
} from '../telemetry/text';
import { validateBootstrap, validateDelivery, validateView } from '../telemetry/validate';

const fixture = fixtureJson as unknown as {
  bootstrap: Record<string, unknown>;
  delivery: Record<string, unknown>;
  invalidCases: Array<{ target: 'view' | 'bootstrap' | 'delivery'; name: string; ops: PatchOp[] }>;
};

type PathPart = string | number;
type PatchOp =
  | { op: 'set'; path: PathPart[]; value: unknown }
  | { op: 'remove'; path: PathPart[] }
  | { op: 'repeat'; path: PathPart[]; count: number };

const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T;

function resolve(
  root: unknown,
  path: PathPart[],
): { parent: Record<string, unknown> | unknown[]; key: PathPart } {
  let node = root as Record<string, unknown> | unknown[];
  for (const part of path.slice(0, -1)) {
    node = (node as Record<string | number, unknown>)[part] as Record<string, unknown> | unknown[];
  }
  const key = path[path.length - 1];
  if (key === undefined) throw new Error('empty patch path');
  return { parent: node, key };
}

function applyOps(document: Record<string, unknown>, ops: PatchOp[]) {
  for (const op of ops) {
    if (op.op === 'repeat') {
      const array = op.path.reduce<unknown>(
        (node, part) => (node as Record<string | number, unknown>)[part],
        document,
      ) as unknown[];
      const first = array[0];
      while (array.length < op.count) array.push(clone(first));
      continue;
    }
    const { parent, key } = resolve(document, op.path);
    if (op.op === 'set') {
      (parent as Record<string | number, unknown>)[key] = op.value;
    } else if (Array.isArray(parent)) {
      parent.splice(Number(key), 1);
    } else {
      delete (parent as Record<string, unknown>)[String(key)];
    }
  }
}

describe('version-1 wire contract', () => {
  it('accepts the canonical bootstrap, view and delivery', () => {
    const bootstrap = validateBootstrap(fixture.bootstrap);
    expect(bootstrap.view.contractVersion).toBe(1);
    expect(bootstrap.gpus).toHaveLength(2);
    expect(validateView(fixture.bootstrap.view).sample?.scopes).toHaveLength(6);
    expect(validateDelivery(fixture.delivery).deliverySequence).toBe(3);
  });

  it('preserves decimal byte strings exactly', () => {
    const view = validateView(fixture.bootstrap.view);
    expect(view.sample?.memory.usedBytes.value).toBe('12025908429');
    expect(parseByteString(view.sample?.memory.usedBytes.value ?? '')).toBe(12025908429n);
  });

  it('rejects every deliberately invalid case', () => {
    expect(fixture.invalidCases.length).toBeGreaterThanOrEqual(30);
    for (const testCase of fixture.invalidCases) {
      const document = clone(
        testCase.target === 'view'
          ? fixture.bootstrap.view
          : testCase.target === 'bootstrap'
            ? fixture.bootstrap
            : fixture.delivery,
      ) as Record<string, unknown>;
      applyOps(document, testCase.ops);
      const validate =
        testCase.target === 'view'
          ? validateView
          : testCase.target === 'bootstrap'
            ? validateBootstrap
            : validateDelivery;
      expect(() => validate(document), testCase.name).toThrow();
    }
  });

  it('accepts stale last-valid values but rejects invalid numeric domains', () => {
    const stale = clone(fixture.bootstrap.view) as {
      sample: { memory: { cachedBytes: { status: string; value: unknown } } };
    };
    expect(validateView(stale).sample?.memory.cachedBytes.value).toBe('4294967296');

    const invalid = clone(fixture.bootstrap.view) as {
      sample: { cpu: { totalPct: { value: unknown } } };
    };
    invalid.sample.cpu.totalPct.value = 100.1;
    expect(() => validateView(invalid)).toThrow(/cpu\.totalPct/);
  });
});

describe('UTF-8 bounds', () => {
  it('truncates at character boundaries', () => {
    expect(truncateUtf8('日本語', 4)).toBe('日');
    expect(truncateUtf8('日本語', 8)).toBe('日本');
    expect(truncateUtf8('日本語', 9)).toBe('日本語');
    expect(truncateUtf8('Kärntner', 4)).toBe('Kär');
    expect(utf8Length('é'.repeat(200))).toBe(400);
    expect(utf8Length(truncateUtf8('é'.repeat(200), MESSAGE_MAX_BYTES))).toBe(160);
  });

  it('strips control characters without touching names', () => {
    expect(stripControl('a\u0000b\nc\u007fd')).toBe('abcd');
    expect(utf8Length('GPU\nName')).toBeLessThanOrEqual(LABEL_MAX_BYTES);
  });
});

describe('decimal byte strings', () => {
  it('accepts the u64 domain only', () => {
    expect(parseByteString('0')).toBe(0n);
    expect(parseByteString('18446744073709551615')).toBe(18446744073709551615n);
    for (const invalid of ['', '0x10', '-1', '+1', ' 1', '1 ', '01', '18446744073709551616']) {
      expect(() => parseByteString(invalid), invalid).toThrow();
    }
  });
});
