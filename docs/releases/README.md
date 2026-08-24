# DataPack Release Notes

DataPack has not made a public product release. This directory defines the
versioned release-note structure without fabricating release history.

For an authorized release, copy `RELEASE-NOTES-TEMPLATE.md` to `v{version}.md`,
replace every placeholder with verified facts, and remove sections that are truly
not applicable. Do not convert planned work into implemented or certified behavior.

Release notes must identify:

- package/application version and release channel;
- source commit and authorized release tag/ref;
- installation artifacts and checksum file;
- supported platforms and Python versions for that release;
- package-versus-archive compatibility facts;
- user-visible changes and migration guidance;
- known limitations and security facts;
- correctness certification evidence; and
- observational performance evidence only when its environment is recorded.

The cumulative change history remains in `CHANGELOG.md`. Artifact names, manifest,
checksum, and smoke-test requirements are defined in
`docs/productization/RELEASE_ARTIFACTS.md`.
