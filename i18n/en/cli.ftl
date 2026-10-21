# SPDX-License-Identifier: Apache-2.0
#
# The words of the command line: what `macro-port` says on the console. A
# message of a language holds its own punctuation around its variables.
# The progress line is the exception: the threads that analyse print it, and
# they do not share the messages. It is a label that the command follows with
# "done / total".

## Errors

cli-error = error: { $message }
cli-error-threads = cannot start the threads of the analysis: { $reason }
cli-error-create = cannot create the directory { $path }: { $reason }
cli-error-write = cannot write the file { $path }: { $reason }
cli-panic = internal error at

## Rules
##
## The problems themselves come from the validators and the core, in English;
## these are the words around them.

cli-rules-file-ok = { $file }: ok
cli-error-rules-read = cannot read { $path }: { $reason }
cli-error-rules-schema = the embedded schema of { $file } is invalid: { $reason }
cli-rules-problem = 1 problem found
cli-rules-problems = { $count } problems found
cli-rules-not-checked = (regexes, ids and references are not checked yet)

## Progress and summary

cli-progress = Files analysed:
cli-summary = Files: { $files } (with macros: { $with_macros }, failed: { $failed })
cli-written = Reports written to { $path }
