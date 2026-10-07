import { describe, expect, it } from 'vitest';
import { describeClose, hostMatches, infoUrl, normalizeRelay, relayAllowed, relayProblem, tunnelUrl } from './relay';

describe('relay urls', () => {
  it('normalises input', () => {
    expect(normalizeRelay('relay.example.com/')).toBe('wss://relay.example.com');
    expect(normalizeRelay('localhost:8080')).toBe('ws://localhost:8080');
    expect(normalizeRelay('https://relay.example.com')).toBe('wss://relay.example.com');
    expect(normalizeRelay('  ')).toBe('');
    expect(normalizeRelay('ws://https://192.168.1.30:8482')).toBe('wss://192.168.1.30:8482');
    expect(normalizeRelay('wss://ws://localhost:8080/')).toBe('ws://localhost:8080');
  });
  it('only allows wss, or ws to this machine', () => {
    expect(relayAllowed('wss://relay.example.com')).toBe(true);
    expect(relayAllowed('ws://localhost:8080')).toBe(true);
    expect(relayAllowed('ws://127.0.0.1:8080')).toBe(true);
    expect(relayAllowed('ws://relay.example.com')).toBe(false);
    expect(relayAllowed('nonsense')).toBe(false);
  });
  it('explains why ws:// to another machine fails', () => {
    expect(relayProblem('ws://192.168.1.30:8482')).toMatch(/relay on 192\.168\.1\.30/);
    expect(relayProblem('wss://192.168.1.30:8482')).toBeNull();
    expect(relayProblem('http://x')).toMatch(/wss:/);
  });
  it('builds tunnel and info urls', () => {
    expect(tunnelUrl('wss://r.example', 'News.Example.com', 563)).toBe('wss://r.example/v1/tcp/news.example.com/563');
    expect(infoUrl('wss://r.example')).toBe('https://r.example/');
    expect(infoUrl('ws://localhost:8080')).toBe('http://localhost:8080/');
  });
});

describe('hostMatches', () => {
  it('matches like the relay', () => {
    expect(hostMatches('*.newshosting.com', 'news.newshosting.com')).toBe(true);
    expect(hostMatches('*.newshosting.com', 'newshosting.com')).toBe(false);
    expect(hostMatches('*.newshosting.com', 'evilnewshosting.com')).toBe(false);
    expect(hostMatches('*', 'anything')).toBe(true);
    expect(hostMatches('news.a.com', 'NEWS.A.COM')).toBe(true);
  });
});

describe('describeClose', () => {
  it('explains relay close codes', () => {
    expect(describeClose(4001, 'h', 1)).toMatch(/doesn't allow h:1/);
    expect(describeClose(4002, 'h', 1)).toMatch(/couldn't reach h:1/);
    expect(describeClose(4003, 'h', 1)).toMatch(/connection limit/);
  });
});
