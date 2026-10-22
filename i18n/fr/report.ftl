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

status-ok = Analysé
status-no-macros = Sans macros
status-encrypted = Chiffré
status-corrupted = Corrompu
status-unsupported = Non pris en charge
status-too-large = Trop volumineux
status-suspicious-archive = Archive suspecte
status-error = Erreur

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
container-text-module = Module texte
container-unknown = Inconnu

## The complexity of a file

level-low = Faible
level-medium = Moyenne
level-high = Élevée

## The severity of a security signal

severity-info = Info
severity-low = Faible
severity-medium = Moyenne
severity-high = Élevée

## Procedures and modules

kind-sub = Sub
kind-function = Function
kind-property-get = Property Get
kind-property-let = Property Let
kind-property-set = Property Set

visibility-public = Publique
visibility-private = Privée
visibility-friend = Friend
visibility-default = Par défaut

module-type-standard = Module standard
module-type-class = Module de classe
module-type-document = Module de document
module-type-form = Formulaire
