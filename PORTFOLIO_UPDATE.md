# Portfolio Update Draft

Here is the suggested new entry and the revised philosophy section.

## 1. New Project Entry (Add to "Featured Implementations")

> **[Project: Edge WASI Runtime](https://github.com/gammahazard/edge-wasi-runtime)**
> A secure, hot-swappable IoT runtime executing sandboxed Python plugins on Raspberry Pi.
> * **Concept:** "The Secure Plugin Host." Running untrusted user scripts (Python) on bare metal without risk.
> * **Stack:** Rust (Host), WASI Component Model, Python (Guest), Tokio.
> * **Innovation:** **Hybrid Architecture**. Rust handles the hardware/network (Safety), Python handles the business logic (Velocity).
> * **Reliability:** Dead-man switches on plugin execution allow the host to survive plugin crashes (e.g., infinite loops or sensor hangs).
> * **Highlight:** Hot-swapping a running sensor driver in <10ms without dropping network connections. **Demonstrated Live on Hardware.**

---

## 2. Updated Engineering Philosophy

*(Replace or appended to the "Engineering Philosophy" section)*

*   **WASM + Docker (The Hybrid Model):** I previously viewed WASM as a Docker replacement. I now architect systems where they coexist: Docker ships the *Infrastructure* (The Rust Host), while WASM ships the *Business Logic* (The Plugins). This enables O(n) secure tenants within O(1) container, drastically reducing overhead compared to "one container per tenant."

---

## 3. Revised "About" Intro (Addressing the Triad)

*(If you remove the "Reliability Triad" table, replace the intro with this streamlined version)*

### Systems Engineer
*Engineering high-assurance systems—from industrial edge devices to enterprise web.*

I don't just build frontends; I engineer complete systems.
My focus is on **Reliability Engineering**—creating applications that look great on the client side while remaining bulletproof on the server side.

**My Core Thesis:**
Composing systems from **Rust** (for correctness), **WASM** (for isolation), and **TypeScript** (for interaction) creates software that fails gracefully and recovers instantly.

---

## 4. Where to fit it?

I recommend placing **Edge WASI Runtime** right after **Protocol Gateway Sandbox**. It complements it perfectly:
1.  **Protocol Gateway**: "I ensure the parser is safe."
2.  **Edge WASI Runtime**: "I ensure the hardware driver is safe."
