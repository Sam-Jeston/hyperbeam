{{- define "hyperbeam.name" -}}
{{- .Release.Name | trunc 50 | trimSuffix "-" -}}
{{- end -}}

{{- define "hyperbeam.labels" -}}
app.kubernetes.io/name: hyperbeam
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ .Chart.Name }}-{{ .Chart.Version }}
{{- end -}}

{{- define "hyperbeam.selector" -}}
app.kubernetes.io/name: hyperbeam
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/component: server
{{- end -}}

{{- define "hyperbeam.secretName" -}}
{{- if .Values.existingSecret -}}{{ .Values.existingSecret }}{{- else -}}{{ include "hyperbeam.name" . }}{{- end -}}
{{- end -}}

{{- define "hyperbeam.serviceHost" -}}
{{ include "hyperbeam.name" . }}.{{ .Release.Namespace }}.svc.cluster.local
{{- end -}}

{{/* Non-empty when the master key lives in AWS rather than in the Secret. */}}
{{- define "hyperbeam.externalMasterKey" -}}
{{- if and .Values.aws.masterKeySecret .Values.aws.kmsKeyId -}}
{{- fail "set only one of aws.masterKeySecret and aws.kmsKeyId" -}}
{{- end -}}
{{- if and .Values.masterKey (or .Values.aws.masterKeySecret .Values.aws.kmsKeyId) -}}
{{- fail "set only one of masterKey, aws.masterKeySecret and aws.kmsKeyId" -}}
{{- end -}}
{{- if or .Values.aws.masterKeySecret .Values.aws.kmsKeyId -}}true{{- end -}}
{{- end -}}
