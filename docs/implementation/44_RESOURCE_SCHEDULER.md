# 44 — Resource Scheduler

The scheduler prevents agents from overwhelming constrained hardware.

Track:

```text
CPU
RAM
GPU/shared memory
disk I/O
inference slots
network
process count
```

Each task receives a resource estimate and priority.

The scheduler must support:

- bounded concurrency;
- backpressure;
- cancellation;
- priority;
- aging to prevent starvation;
- resource reservations;
- adaptive throttling.

On CPU-first laptops, default toward one primary inference workload and limited background activity. The actual limits must be discovered from the machine rather than hard-coded.

When memory pressure rises:

```text
stop background loads
 → pause low-priority tasks
 → unload idle models
 → compact caches
 → reduce concurrency
 → resume safely
```
