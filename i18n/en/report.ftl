# SPDX-License-Identifier: Apache-2.0
#
# The words of a report. A message is named after an enumeration of
# schema/report.schema.json and one of its values, with `-` in place of `_`:
# `status-no-macros` is the status `no_macros`. A test fails when a value has no
# message, in either language.
#
# The labels of families, signals and traits are not here: they are the `label`
# fields of the rules.

## The outcome of the analysis of a file

status-ok = Analysed
status-no-macros = No macros
status-encrypted = Encrypted
status-corrupted = Corrupted
status-unsupported = Unsupported
status-too-large = Too large
status-suspicious-archive = Suspicious archive
status-error = Error

## The format of a file

container-xlsm = XLSM
container-xltm = XLTM
container-xlam = XLAM
container-xlsb = XLSB
container-xls = XLS
container-xlt = XLT
container-xla = XLA
container-xlsx = XLSX
container-xltx = XLTX
container-text-module = Text module
container-unknown = Unknown

## The complexity of a file

level-low = Low
level-medium = Medium
level-high = High

## The severity of a security signal

severity-info = Info
severity-low = Low
severity-medium = Medium
severity-high = High

## Procedures and modules

kind-sub = Sub
kind-function = Function
kind-property-get = Property Get
kind-property-let = Property Let
kind-property-set = Property Set

visibility-public = Public
visibility-private = Private
visibility-friend = Friend
visibility-default = Default

module-type-standard = Standard module
module-type-class = Class module
module-type-document = Document module
module-type-form = Form
