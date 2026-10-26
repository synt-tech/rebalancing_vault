#!/usr/bin/env python3
"""Convert `stellar snapshot create` output (diagnostic XDR-JSON) into the
canonical stellar-xdr serde JSON that `soroban-ledger-snapshot` /
`Env::from_ledger_snapshot_file` expects.

CLI emits:   ledger_entries: [ {"entry": <LedgerEntry>, "live_until": <u32>} ]
SDK expects: ledger_entries: [ [<LedgerKey>, [<LedgerEntry>, <live_until?>]] ]

ScVal/entry tags already match canonical snake_case serde; only the
integer payloads differ:

  {"u32"|"i32"|"u64"|"i64": "N"}  -> {"<tag>": N}
  {"u128"|"i128": "N"}            -> {"<tag>": {"hi": H, "lo": L}}
  {"u256"|"i256": "N"}            -> {"<tag>": {"hi_hi":..,"hi_lo":..,
                                              "lo_hi":..,"lo_lo":..}}

Addresses keep strkey form (ScAddress is Display-serialized); bytes are hex.

`stellar snapshot create` does not emit contract_code or classic account
entries; pass each executable's `hash:path` (fetched via `stellar contract
fetch`) to append code, and `--account GADDR:FLAGS` to synthesize a minimal
issuer account (SAC `is_asset_auth_required` reads issuer account flags).

Usage: convert.py in.json out.json [--account GADDR:FLAGS ...] [hash:wasmfile ...]
"""
import json
import sys


def parts128(v: int, signed: bool) -> dict:
    lo = v & ((1 << 64) - 1)
    hi = v >> 64
    if signed and hi >= (1 << 63):
        hi -= 1 << 64
    return {"hi": hi, "lo": lo}


def parts256(v: int, signed: bool) -> dict:
    m = (1 << 64) - 1
    lo_lo, lo_hi, hi_lo, hi_hi = (v & m, (v >> 64) & m,
                                 (v >> 128) & m, (v >> 192) & m)
    if signed and hi_hi >= (1 << 63):
        hi_hi -= 1 << 64
    return {"hi_hi": hi_hi, "hi_lo": hi_lo, "lo_hi": lo_hi, "lo_lo": lo_lo}


def convert(o):
    if isinstance(o, list):
        return [convert(v) for v in o]
    if not isinstance(o, dict):
        return o
    if len(o) == 1:
        tag, v = next(iter(o.items()))
        if tag in ("u32", "i32", "u64", "i64") and isinstance(v, str):
            return {tag: int(v)}
        if tag in ("u128", "i128") and isinstance(v, str):
            return {tag: parts128(int(v), tag == "i128")}
        if tag in ("u256", "i256") and isinstance(v, str):
            return {tag: parts256(int(v), tag == "i256")}
    return {k: convert(v) for k, v in o.items()}


def ledger_key(entry: dict) -> dict:
    data = entry["data"]
    kind, v = next(iter(data.items()))
    if kind == "contract_data":
        return {"contract_data": {
            "contract": v["contract"], "key": convert(v["key"]),
            "durability": v["durability"]}}
    if kind == "contract_code":
        return {"contract_code": {"hash": v["hash"]}}
    if kind == "account":
        return {"account": {"account_id": v["account_id"]}}
    if kind == "trustline":
        return {"trustline": {"account_id": v["account_id"],
                              "asset": v["asset"]}}
    raise ValueError(f"unhandled ledger entry kind: {kind}")


def main() -> None:
    src, dst = sys.argv[1], sys.argv[2]
    d = json.load(open(src))
    out = {k: d[k] for k in d if k != "ledger_entries"}
    # Local env-host may not support the live protocol version; clamp it.
    out["protocol_version"] = min(int(out.get("protocol_version", 0)), 29)
    out["ledger_entries"] = [
        [ledger_key(e["entry"]), [convert(e["entry"]), e.get("live_until")]]
        for e in d["ledger_entries"]
    ]
    seq = d["sequence_number"]
    for spec in sys.argv[3:]:
        if spec.startswith("--account="):
            gaddr, flags = spec[10:].rsplit(":", 1)
            entry = {"last_modified_ledger_seq": seq,
                     "data": {"account": {
                         "account_id": gaddr, "balance": 1000000000000,
                         "seq_num": 0, "num_sub_entries": 0,
                         "inflation_dest": None, "flags": int(flags),
                         "home_domain": "", "thresholds": "01000000",
                         "signers": [], "ext": "v0"}},
                     "ext": "v0"}
            out["ledger_entries"].append(
                [{"account": {"account_id": gaddr}}, [entry, 4294967295]])
            print(f"injected account {gaddr[:12]}… flags={flags}")
            continue
        h, path = spec.split(":", 1)
        entry = {"last_modified_ledger_seq": d["sequence_number"],
                 "data": {"contract_code": {"ext": "v0", "hash": h,
                          "code": open(path, "rb").read().hex()}},
                 "ext": "v0"}
        out["ledger_entries"].append(
            [{"contract_code": {"hash": h}}, [entry, 4294967295]])
        print(f"injected wasm {h[:12]}… ({path})")
    json.dump(out, open(dst, "w"))
    print(f"{dst}: {len(out['ledger_entries'])} entries")


if __name__ == "__main__":
    main()
