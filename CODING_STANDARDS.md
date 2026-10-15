# subsidy Coding Standards

This document defines the review invariants and architectural standards for the `subsidy` codebase. It is consulted by reviewer agents and humans during code review.

## Review Invariants

- **Relocation Invariance**: When relocating domain logic, helper routines, or unit tests between modules, preserve all specification references (e.g. section numbers) and explanatory test comments verbatim.
- **Deep Modules**: Prefer rich domain value objects with small, clean interfaces. Keep helper routines private unless cross-module access is strictly required.
- **Value Object Boundary Integrity**: When domain logic is encapsulated in a value object (e.g. `MatchDocNo`, `UniqueIndex`, `UploadedStatusIndex`), pass the value object or its borrow directly to callers. Do not expose bridge methods that degrade domain models back into primitive collections (`HashMap`, tuples) to avoid updating call sites.
- **Private Interface Guard**: Internal record types (`UnionPayRecord`, `InvoiceRecord`, `ReceiptRecord`, `MultiValueIndex`, `UniqueIndex`, `SalesCouponReconciler`) must remain `pub(crate)`; public context structs expose only crate-boundary safe methods.
- **Zero-Clone Pipeline Execution**: Pipeline jobs take ownership or borrow domain tables (`take_refund_table`, `take_uploaded_table`, `&Table`). `Table` intentionally does not implement `Clone` to eliminate heap duplication at compile time.
- **Domain Types Over Primitive Tuples/Arrays**: Avoid naked integer/boolean arrays or tuples to represent domain states or breakdown counters; use named structs or domain enums (e.g. `UnuploadedBreakdown`, `FinalMatchUnmatched`, `StatusAmountCount`).
- **Builder Coordinate Encapsulation**: Layout and worksheet builders (`ReportSheetBuilder`) must track internal coordinates and ranges. Callers must not calculate row offsets (`cur_row() - 1`) or juggle raw coordinate pairs; multi-row operations requiring formatting must be exposed as builder batch methods (e.g. `write_rows_with_unique_reference_highlight`).
- **Domain Enums Over Unstructured Extra Fields**: Represent variant-specific failure reasons or auxiliary context as domain enums with typed variants (e.g. `FailedUploadExtra`), rather than generic string fields (`extra1`, `extra2`).
