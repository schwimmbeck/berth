.PHONY: docs docs-serve deny bench-smoke

docs:
	mdbook build docs

docs-serve:
	mdbook serve docs --open

deny:
	cargo deny check

bench-smoke:
	bash scripts/benchmark-smoke.sh 25
