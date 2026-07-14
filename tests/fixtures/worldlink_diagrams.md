# WorldLink failure-handling diagrams (fixture)

Minimal extract used to regression-test that pathological Mermaid in this
document cannot hang or crash the pager. Full plan lives outside this repo.

```mermaid
flowchart TD
    A[CitiConnect call: submit or inquiry] --> B{Response}
    B -->|"HTTP 2xx, ACCEPTED"| OK[Continue normal lifecycle]
    B -->|"timeout / HTTP 5xx / polling timeout"| TECH[Technical failure]
    B -->|"RJCT / CAMT.056 bank return / validation reject"| BIZ[Business failure]
    TECH --> MRR["Status = ManualRetryRequired<br/>(FNM1-1524)"]
    BIZ --> RR["Status = Rejected / Returned<br/>(FNM1-1522)"]
    MRR -.->|"data is fine - resend same instruction"| RETRY[Manual Retry]
    RR -.->|"data was wrong - new instruction"| RESUB[Resubmit]
```

```mermaid
stateDiagram-v2
    [*] --> Processing: instruction sent to Citi
    Processing --> Credited: success
    Processing --> ManualRetryRequired: technical failure (1524)
    Processing --> Returned: bank return CAMT.056 (1522)
    Processing --> Rejected: Citi RJCT (1522)

    ManualRetryRequired --> Processing: Retry (same instruction, no changes)
    ManualRetryRequired --> ManualRetryRequired: Retry fails again

    Rejected --> NewBatch: Resubmit (new instruction, live WSE bank details)
    Returned --> NewBatch: Resubmit (new instruction, live WSE bank details)
    NewBatch --> Processing: Authorize -> Book FX -> Send

    Credited --> [*]
    note right of Rejected
        Original failure row is preserved
        in Audit Log + Pay Ledger (AC6)
    end note
```

```mermaid
sequenceDiagram
    actor U as Treasury user
    participant FE as Payment Tracking (tracking.tsx)
    participant API as DisbursementController
    participant WF as DisbursementWorkflowService
    participant Citi as CitiClient
    participant Audit as PaymentAuditLog

    Note over FE: Row shows ManualRetryRequired -> Retry button visible
    U->>FE: Click Retry
    FE->>API: POST /disbursement/{internationalPaymentId}/retry (role-gated)
    API->>Audit: ManualRetryTriggered
    API->>WF: resend stored instruction (no bank/FX/auth change)
    WF->>Citi: InitiatePaymentAsync (same payload)
    Citi-->>WF: accepted / failed
    alt success
        WF-->>API: status -> Processing
        API->>Audit: ManualRetrySucceeded
    else failure
        WF-->>API: status -> ManualRetryRequired
        API->>Audit: ManualRetryFailed
    end
    API-->>FE: result -> banner + refreshed status + log outcome
```

```mermaid
sequenceDiagram
    actor U as Payroll user
    participant FE as Review Payments (review.tsx)
    participant API as DisbursementController
    participant WF as DisbursementWorkflowService
    participant WSE as WSE Profile
    participant Audit as PaymentAuditLog

    Note over FE: Row shows Rejected/Returned -> Resubmit button visible
    U->>FE: Click Resubmit
    FE->>API: POST /disbursement/{takeHomePayId}/resubmit (role-gated)
    API->>WSE: fetch LIVE bank account / SWIFT
    Note over API,WF: keep net / ccy / purpose / employeeId LOCKED
    API->>WF: new Payment Instruction ID + new standalone batch
    API->>Audit: Resubmitted (original Rejected/Returned event preserved)
    WF-->>API: new batch created (Awaiting Approval)
    API-->>FE: banner; new batch appears in Disburse Pay -> Authorize -> Book FX -> Send
```
