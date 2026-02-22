.PHONY: docs docs-serve deny

docs:
	mdbook build docs

docs-serve:
	mdbook serve docs --open

deny:
	cargo deny check
