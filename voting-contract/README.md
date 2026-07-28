# A Ballot Contract for Injective — beginner's walkthrough

This is Solidity's famous `Ballot.sol` teaching contract, rewritten in Rust for CosmWasm and deployable on Injective.

If you have never written a smart contract before, start at the top and keep reading. Nothing here assumes you know Rust or Cosmos.

---

## 1. What the contract does

Think of a committee vote.

- Whoever **deploys** the contract becomes the **chairperson**, and gets one vote.
- The chairperson **hands out the right to vote**, one address at a time. Each grant is worth one vote.
- Each voter then does exactly one of two things, once, permanently:
  - **votes** for a proposal, or
  - **delegates** their vote to someone they trust.
- Anyone can **read the totals** for free, at any time.

That's it. Five actions, mirroring the diagram this was built from:

```
                    instantiate()
              deploys, stores proposals
                          |
        ┌─────────────────▼──────────────────┐
        │ CHAIRPERSON ONLY                   │
        │   give_right_to_vote(voter)        │
        │   grants weight = 1                │
        └─────────────────┬──────────────────┘
                          |
        ┌─────────────────▼──────────────────┐
        │ WHITELISTED VOTERS                 │
        │   vote(proposal)   delegate(to)    │
        │   adds weight      passes weight   │
        └─────────────────┬──────────────────┘
                          |
        ┌─────────────────▼──────────────────┐
        │ ANYONE — free reads                │
        │   winning_proposal()  winner_name()│
        └────────────────────────────────────┘
```

---

## 2. The one big idea: three entry points

A Solidity contract exposes many public functions. **A CosmWasm contract exposes exactly three:**

| Entry point | When it runs | Costs gas? | Can it write? |
|---|---|---|---|
| `instantiate` | once, at deploy | yes | yes |
| `execute` | every state-changing call | yes | yes |
| `query` | every read | **no** | **no** |

So how do you have five different actions with only three entry points? **You send a JSON message that names the action.** `execute` looks at the message and dispatches.

```json
{ "give_right_to_vote": { "voter": "inj1abc..." } }
{ "vote":                { "proposal": 0 } }
{ "delegate":            { "to": "inj1xyz..." } }
```

Those three JSON shapes are the `ExecuteMsg` enum in `src/msg.rs`. Rust enum variant → snake_case JSON key. That's the whole trick.

Queries work the same way, but are free — no transaction, no signature, no gas:

```json
{ "proposals": {} }
{ "winner_name": {} }
{ "voter": { "address": "inj1abc..." } }
```

---

## 3. Coming from Solidity? Here's the map

| Solidity | CosmWasm | Where |
|---|---|---|
| `constructor(names)` | `instantiate` | `contract.rs` |
| `function giveRightToVote(address)` | `ExecuteMsg::GiveRightToVote` | `msg.rs` |
| `function vote(uint)` | `ExecuteMsg::Vote` | `msg.rs` |
| `function delegate(address)` | `ExecuteMsg::Delegate` | `msg.rs` |
| `function winnerName() view` | `QueryMsg::WinnerName` | `msg.rs` |
| `msg.sender` | `info.sender` | every handler |
| `msg.value` | `info.funds` (a list of coins) | unused here |
| `block.number` | `env.block.height` | unused here |
| `address public chairperson` | `Item<Addr>` named `"chairperson"` | `state.rs` |
| `mapping(address => Voter)` | `Map<&Addr, Voter>` | `state.rs` |
| `require(cond, "msg")` | `return Err(ContractError::X {})` | `error.rs` |
| `revert` | returning any `Err` | everywhere |
| events | `Response::add_attribute` | every handler |
| `address(0)` means "unset" | `Option<Addr>` → `None` | `state.rs` |

Two differences worth internalising:

**Storage is explicit.** Solidity gives you storage slots automatically. CosmWasm makes you declare a named accessor: `Item` for one value, `Map` for a key-value collection. You then call `.load()` / `.save()` on it. More typing, but you always know exactly what touches the chain.

**"Not found" is a real case.** A Solidity mapping happily returns a zeroed struct for an address it has never seen. CosmWasm's `.may_load()` returns `None`, and Rust forces you to handle it. That's why you'll see `.unwrap_or_default()` in a few places — that is the deliberate choice to treat "unknown address" as "a voter with weight 0".

---

## 4. The tricky part: delegation

Voting is simple. Delegation has one subtlety that trips people up.

Suppose **Bob already delegated to Carol**. Now **Alice delegates to Bob**. Where does Alice's vote go?

It must go to **Carol** — not Bob. Bob has already used his turn; a vote parked on his record would never be cast. So the contract **follows the chain to the end** before depositing the weight:

```
Alice ──delegates to──▶ Bob ──already delegated to──▶ Carol
                                                        ▲
                          Alice's weight lands here ────┘
```

There's a second case. If the person at the end of the chain **has already voted**, their weight is spent too — so the incoming weight goes *directly onto the proposal they chose*, immediately bumping the count.

And a third: what if **A delegates to B, and B delegates back to A**? Following that chain would loop forever. The contract detects it and rejects the transaction (`DelegationLoop`). It also caps the chain at 20 hops, so no one can build a chain so long it exhausts gas.

All three cases are covered by tests you can read:
- `delegation_moves_weight_to_someone_who_has_not_voted`
- `delegating_to_someone_who_already_voted_counts_immediately`
- `delegation_chains_collapse_to_the_final_delegate`
- `delegation_loops_are_rejected`

---

## 5. The files

| File | What's in it | Read it when |
|---|---|---|
| `src/msg.rs` | the public API — every message in and out | first |
| `src/state.rs` | what gets stored on-chain | second |
| `src/contract.rs` | all the logic | third |
| `src/error.rs` | every way a call can be rejected | as needed |
| `tests/integration.rs` | 20 tests against a simulated chain | to see it work |
| `src/bin/schema.rs` | generates JSON schemas for frontends | rarely |

Every file has comments explaining the *why*, not just the *what*.

---

## 6. Run it yourself

You need Rust. Nothing else — no node, no Docker, no wallet.

```shell
cargo test
```

That runs 20 tests against `cw-multi-test`, an in-memory blockchain. It takes about a second and exercises the real contract code. Try breaking a rule in `src/contract.rs` and watch which test catches you.

Other commands:

```shell
cargo wasm     # compile to WebAssembly
cargo schema   # regenerate schema/ (JSON schemas for frontends)
cargo clippy   # Rust's linter
```

---

## 7. Deploying to Injective testnet

Only when you're ready to put it on a real chain.

### Step 1 — build an optimized wasm

The plain `cargo wasm` output (~350 KB) is **not deployable** — chains enforce a size limit. Use the official optimizer (needs Docker):

```shell
docker run --rm -v "$(pwd)":/code \
  --mount type=volume,source="$(basename "$(pwd)")_cache",target=/target \
  --mount type=volume,source=registry_cache,target=/usr/local/cargo/registry \
  cosmwasm/optimizer:0.16.1
```

Output lands in `artifacts/voting_contract.wasm` (~200 KB). On Apple Silicon you can use `cosmwasm/optimizer-arm64:0.16.1`, but be aware it produces a **different checksum** than the x86 image — use x86 if the hash needs to be reproducible.

### Step 2 — get `injectived` and testnet funds

Install `injectived`, create a key, then get free testnet INJ from https://testnet.faucet.injective.network

### Step 3 — upload, deploy, use

```shell
NODE=https://testnet.sentry.tm.injective.network:443
CHAIN=injective-888
FLAGS="--chain-id=$CHAIN --node=$NODE --gas-prices=500000000inj --gas=auto --gas-adjustment=1.4 -y -b sync"

# upload the code (gives you a CODE_ID)
injectived tx wasm store artifacts/voting_contract.wasm --from=<key> $FLAGS

# create a ballot from that code (gives you a CONTRACT address)
injectived tx wasm instantiate <CODE_ID> \
  '{"proposals":["Alpha","Beta","Gamma"]}' \
  --label="ballot-v1" --admin=<your-inj-address> --from=<key> $FLAGS

# chairperson lets someone vote
injectived tx wasm execute <CONTRACT> \
  '{"give_right_to_vote":{"voter":"inj1..."}}' --from=<key> $FLAGS

# that someone votes for proposal 0
injectived tx wasm execute <CONTRACT> '{"vote":{"proposal":0}}' --from=<their-key> $FLAGS

# anyone reads the result — free, no key needed
injectived query wasm contract-state smart <CONTRACT> '{"winner_name":{}}' --node=$NODE
```

Upload and instantiate are two separate steps on purpose: one uploaded `CODE_ID` can back many independent ballots.

**Mainnet** is `injective-1` at `https://sentry.tm.injective.network:443`, but note that **mainnet code upload is permissioned** — it requires a governance proposal, not a plain `tx wasm store`. Learn on testnet.

---

## 8. Safety choices worth knowing

Small decisions in the code that matter, and why:

- **Every address string is validated** with `addr_validate` before use. An unchecked string could be a typo you can never undo, or an address for a different chain entirely.
- **Vote counts use `checked_add`**, which errors instead of silently wrapping to zero on overflow. Wrapping arithmetic has cost real contracts real money.
- **Re-granting the vote is rejected.** A second `give_right_to_vote` on the same address would reset their weight to 1 — quietly destroying any votes that had been delegated to them.
- **Duplicate proposal names are rejected at deploy.** Otherwise "Alpha won" is ambiguous.
- **Ties are reported, not hidden.** The queries return `tied: true` when there is no single leader. The Solidity original silently returns the first of the tied proposals with no indication anything was ambiguous — a real trap.
- **The delegation chain is capped at 20 hops**, so nobody can build a chain long enough to exhaust gas for everyone downstream.

---

## 9. Deliberately left out

This is a teaching contract. If you take it further, these are the real gaps:

- **No deadline.** Voting never closes, so `winner_name` is only ever "the leader so far". Add a block-height or timestamp cutoff.
- **The chairperson is trusted and permanent.** They choose the entire electorate and cannot be replaced.
- **Nothing happens when a proposal wins.** The contract counts; it does not act. Real governance dispatches a `CosmosMsg` on success.
- **No migration.** The `cw2` version is stored but there is no `migrate` entry point, so the code cannot be upgraded in place.
- **One address, one vote.** Fine for a committee, trivially sybil-attackable for anything open. Real DAOs weight by token balance or staked amount.
