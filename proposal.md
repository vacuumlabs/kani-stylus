# **kani-stylus: Bounded Model Checking and Formal Verification Harness for Arbitrum Stylus**

**Author:** Independent Protocol Engineer (Hackathon Track)  
**Target Ecosystem:** Arbitrum Stylus / Orbit Chains

**Track Alignment:** Developer Tooling, Formal Verification, Protocol Security

**Post-Hackathon Funding Target:** Arbitrum Stylus Sprint Category C / Arbitrum DAO Domain Allocator Offerings

## **1\. Executive Summary**

Arbitrum Stylus allows smart contracts written in Rust to execute alongside EVM bytecode within ArbOS, achieving up to 100x efficiency gains for computational logic while sharing an identical, co-equal 256-bit storage trie. However, the formal verification tooling landscape remains focused on Solidity, utilizing engines such as Halmos, Certora, and HEVM.

Rust smart contracts compiled to WebAssembly (wasm32-unknown-unknown) cannot currently leverage Rust-native formal verification engines like Kani (an SMT-backed bounded model checker developed by Amazon Web Services using the CBMC solver). When Kani evaluates a Stylus contract, execution fails immediately because the stylus-sdk depends on low-level ArbOS foreign function interface (FFI) declarations (extern "C") that are provided dynamically by the node runtime at onchain activation.

kani-stylus solves this execution boundary deficit by introducing a symbolic abstraction crate (kani-stylus-core) that mocks ArbOS host calls with bounded, symbolic in-memory models. This enables protocol engineers to formally prove functional invariants—such as token balance conservation, arithmetic overflow absence, and access control enforceability—across the entire input space without manual test vector generation.

## **2\. Problem Statement and Architectural Context**

### **The Host-Call Execution Barrier**

In Stylus, smart contracts interface with ArbOS through low-level C externs:

> * Reading transaction calldata via read\_args  
> * Returning data buffers via write\_result  
> * Reading and updating state via storage\_load\_bytes32 and storage\_cache\_bytes32  
> * Inspecting transaction parameters via msg\_sender, msg\_value, and block context

When Kani attempts symbolic execution across native Rust contract code, it encounters unresolved external symbols or uninitialized memory pointers at the boundary of these extern calls. Without an SMT-compatible translation layer, bounded model checking crashes before reaching core contract business logic.

### **Silent Runtime Failures in Stylus**

Unlike standard Rust binaries, Stylus contracts compile under \#\!\[no\_std\] and enforce strict execution determinism:

> 1. **Unstructured Panics:** Rust panic\!() invocations cause Stylus contracts to abort immediately, consuming remaining gas and returning opaque errors to callers instead of clean ABI-encoded revert reasons.

> 2. **Arithmetic Inconsistencies:** Subtle rounding behaviors and 256-bit integer conversions can result in state drift or vulnerability patterns across complex DeFi pricing models.

> 3. **Privilege Leaks:** Complex inheritance and module structures in Rust contract libraries can result in administrative endpoints omitting caller authentication checks.

kani-stylus provides a deterministic symbolic execution framework that discovers counterexamples for these failure modes before deployment.

## **3\. Technical Architecture and Design**

kani-stylus is architected as an extensible verification harness comprising two key components: the symbolic runtime harness and the verification proof interface.

| System Component | Rust Module | Functionality & Abstraction |
| :---- | :---- | :---- |
| **Symbolic Host Stubs** | kani\_stylus\_core::host | Intercepts extern "C" declarations and substitutes them with bounded in-memory SMT stubs.  |
| **Symbolic Storage Trie** | kani\_stylus\_core::storage | Mocks the EVM 256-bit key-value store using a bounded symbolic array returning kani::any().  |
| **Execution Context Injector** | kani\_stylus\_core::context | Supplies non-deterministic symbolic callers, chain IDs, timestamps, and calldata.  |
| **Verification Macro** | kani\_stylus::proof | Generates the CBMC proof harness and binds contract structs to symbolic memory.  |

### **Symbolic Storage Model**

The EVM storage model in Stylus maps 256-bit slot keys to 256-bit slot values. Under kani-stylus, storage reads and writes are mapped to an SMT-constrained symbolic associative array:

> * When a slot is accessed via storage\_load\_bytes32, the harness evaluates whether that slot has been written to during the current symbolic execution path.  
> * If unwritten, it generates a symbolic variable using kani::any::\<\[u8; 32\]\>() constrained by any user-defined precondition assumptions (kani::assume).  
> * When state is modified via storage\_cache\_bytes32, the symbolic map records the symbolic state update, accurately tracking cross-slot state dependencies.

### **Invariant Verification Mechanism**

The verifier evaluates three classes of contract invariants without requiring concrete unit test inputs:

> 1. **Arithmetic Conservation:** For token contracts, proving that transfers strictly preserve total balance invariants:  
>    $$\\text{balance}(A)\_{\\text{post}} \+ \\text{balance}(B)\_{\\text{post}} \= \\text{balance}(A)\_{\\text{pre}} \+ \\text{balance}(B)\_{\\text{pre}}$$  
>    across all values of $A$, $B$, and amounts without arithmetic wrap-around.

> 2. **Access Control Enforcement:** Proving that administrative endpoints unconditionally trigger an execution rollback or return an unauthorized error when msg::sender() does not match the stored owner address.

> 3. **Panic Freedom:** Verifying that no arbitrary sequence of calldata bytes can induce an unexpected Rust panic\!() in public ABI functions.

## **4\. Minimum Viable Product (MVP) Scope (48–72 Hours)**

The hackathon MVP is engineered strictly for single-developer execution within a 2-to-3-day sprint. It focuses exclusively on stubbing critical host calls and proving properties over a standard Stylus ERC-20 implementation.

### **In-Scope Hackathon Deliverables**

> * **kani-stylus-core Crate:** Stub implementations for the five most critical Stylus ArbOS host operations:  
  * read\_args (symbolic calldata buffer)  
  * write\_result (symbolic return buffer validation)  
  * storage\_load\_bytes32 (symbolic storage read)  
  * storage\_cache\_bytes32 (symbolic storage write)  
  * msg\_sender (symbolic 20-byte caller address)

> * **Two Formal Verification Proofs:**  
  * Proof 1: *ERC-20 Transfer Conservation Proof* confirming zero balance leakage across all permutations.

  * Proof 2: *Ownable Access Control Proof* verifying that unauthorized callers cannot access protected endpoints.

> * **Defect Injection and Counterexample Verification:** Demonstrating that Kani detects intentionally inserted arithmetic and access control vulnerabilities and outputs concrete counterexample traces.

> * **Automated Test Runner:** A single script or cargo alias executing cargo kani across the target harnesses with human-readable CLI summaries.

### **Explicitly Out-of-Scope for the MVP**

> * Simulating external contract-to-contract call dispatch (stylus\_sdk::call).  
> * Full symbolic parsing of complex ABI dynamic strings and nested byte arrays.  
> * Static analysis AST linting rules (relegated to future tooling pipelines).

## **5\. Step-by-Step Implementation Roadmap**

| Timeline | Phase Focus | Key Tasks & Technical Milestones |
| :---- | :---- | :---- |
| **Hours 00–12** | Core Stubbing & Architecture | Initialize \#\!\[no\_std\] crate; implement symbolic stubs for read\_args, write\_result, msg\_sender, and storage primitives using kani::any().  |
| **Hours 12–24** | Symbolic Storage Engine | Implement bounded symbolic storage mapping supporting EVM slot key hashing; verify that storage writes and reads preserve symbolic dependencies.  |
| **Hours 24–36** | Harness Construction & ERC-20 Proofs | Implement OpenZeppelin Stylus ERC-20 proof harnesses; assert balance conservation and access control constraints; verify proof convergence in Kani.  |
| **Hours 36–48** | Defect Validation & Documentation | Inject intentional overflow and access control flaws; verify counterexample generation; package code, CLI execution script, demo video, and grant proposal draft.  |

## **6\. Ecosystem Alignment and Grant Trajectory**

### **Strategic Importance to Arbitrum**

Formal verification is standard practice for high-value EVM smart contracts but is a critical missing link for Arbitrum Stylus. By enabling native formal verification for Rust smart contracts, kani-stylus directly reduces the security barrier preventing established Ethereum and Solana protocols from deploying mission-critical infrastructure to Arbitrum.

### **Grant Alignment**

> * **Arbitrum Stylus Sprint (RFP Category C: "Enhanced Debugging Workflows and Tooling"):** The project satisfies the mandate to deliver advanced debugging and security verification tooling for Stylus smart contracts.

> * **Arbitrum DAO Domain Allocator Offerings (Developer Tooling Track):** Questbook-managed grant tracks provide up to $25,000 to $50,000 USDC for open-source developer tooling and testing infrastructure.

### **Long-Term Post-Hackathon Roadmap**

> 1. **Phase 1 (Months 1–2):** Add support for symbolic cross-contract calls and reentrancy property proofs.

> 2. **Phase 2 (Months 3–4):** Build verification harnesses for storage layout compatibility between Solidity proxy contracts and Stylus logic implementations.

> 3. **Phase 3 (Months 5–6):** Package the verification suite as a GitHub Actions CI workflow for continuous automated verification of Stylus repositories on every pull request.  
