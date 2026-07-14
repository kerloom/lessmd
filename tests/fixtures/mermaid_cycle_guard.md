# Mermaid cycle-guard fixture

Synthetic diagrams that exercise cycle refusal without hanging the pager.
The cyclic state diagram must fail fast; the others should still render.

```mermaid
flowchart TD
    A[Start] --> B{Branch}
    B -->|ok| C[Done]
    B -->|retry| A
```

```mermaid
%% leading comment must not bypass cycle detection
%%{init: {'theme': 'dark'}}%%
stateDiagram-v2
    [*] --> Ready
    Ready --> Working
    Working --> Ready: loop
    Working --> Done
    Done --> [*]
    note right of Working
        Notes may mention A --> B without
        creating a real transition edge.
    end note
```

```mermaid
sequenceDiagram
    participant A as Alice
    participant B as Bob
    A->>B: hello
    B-->>A: world
```
