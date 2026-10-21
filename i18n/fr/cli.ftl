# SPDX-License-Identifier: Apache-2.0
#
# The words of the command line: what `macro-port` says on the console. A
# message of a language holds its own punctuation around its variables.
# The progress line is the exception: the threads that analyse print it, and
# they do not share the messages. It is a label that the command follows with
# "done / total".

## Errors

cli-error = erreur : { $message }
cli-error-threads = impossible de démarrer les threads de l'analyse : { $reason }
cli-error-create = impossible de créer le répertoire { $path } : { $reason }
cli-error-write = impossible d'écrire le fichier { $path } : { $reason }
cli-panic = erreur interne à

## Progress and summary

cli-progress = Fichiers analysés :
cli-summary = Fichiers : { $files } (avec macros : { $with_macros }, en échec : { $failed })
cli-written = Rapports écrits dans { $path }
