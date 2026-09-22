## Objective
Refine spec `18b6a9c5` with hardware-adaptive latency expectations.

## Context
Owner decision: “classify hardware strength from CPU/RAM speed; scale the expected latency baseline plus tolerance by that classification. Adaptive expectation across machines, enforcing target latency relative to hardware capacity.”
Audit: `tmp/test-coverage-audit/03-requirements.md`.

## Acceptance criteria
- Define hardware classes and measurement inputs.
- Define scaled baseline and tolerance calculation.
- Define reporting for unsupported/unknown hardware.

## Out of scope
- Building the performance harness.