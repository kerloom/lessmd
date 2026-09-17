# Parallel Processing

```mermaid
stateDiagram-v2
    state Split <<fork>>
    state Merge <<join>>
    [*] --> Split
    Split --> First
    Split --> Second
    First --> Merge
    Second --> Merge
    Merge --> [*]
```
